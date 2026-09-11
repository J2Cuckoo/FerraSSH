use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle, KeyboardInteractiveAuthResponse};
use russh::keys::{decode_secret_key, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey};
use russh::{Channel, ChannelMsg, Disconnect};
use tokio::sync::Mutex;

use crate::algs::AlgSpec;
use crate::store::{AuthMethodKind, KnownHost, SessionSecret};
use crate::{Error, Result};

#[derive(Clone)]
pub struct ConnectOpts {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethodKind,
    pub secret: SessionSecret,
    pub private_key_pem: Option<String>,
    pub algs: AlgSpec,
    pub expected_fingerprint: Option<String>,
    pub accept_unknown_host: bool,
    pub keepalive: u64,
    pub compression: bool,
    pub jump: Option<Box<ConnectOpts>>,
}

pub struct Established {
    pub handle: Arc<Mutex<Handle<FerraHandler>>>,
    pub fingerprint: String,
    pub algorithm: String,
}

pub struct FerraHandler {
    discovered: Arc<Mutex<Option<(String, String)>>>,
}

impl FerraHandler {
    fn new() -> (Self, Arc<Mutex<Option<(String, String)>>>) {
        let discovered = Arc::new(Mutex::new(None));
        (Self { discovered: discovered.clone() }, discovered)
    }
}

impl client::Handler for FerraHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // Always continue KEX so we can show the fingerprint and do TOFU in finish_connect.
        // Returning false here makes russh abort with a bare "Unknown server key".
        let fp = format!("{}", server_public_key.fingerprint(HashAlg::Sha256));
        let alg = server_public_key.algorithm().to_string();
        *self.discovered.lock().await = Some((fp, alg));
        Ok(true)
    }
}

pub async fn connect(opts: ConnectOpts) -> Result<Established> {
    if let Some(jump) = opts.jump.clone() {
        let bastion = Box::pin(connect(*jump)).await?;
        let channel = {
            let h = bastion.handle.lock().await;
            h.channel_open_direct_tcpip(&opts.host, opts.port as u32, "127.0.0.1", 0).await?
        };
        return connect_stream(opts, channel.into_stream(), Some(bastion)).await;
    }
    let addr = format!("{}:{}", opts.host, opts.port);
    let (handler, discovered) = FerraHandler::new();
    let config = client_config(&opts)?;
    let handle = client::connect(Arc::new(config), addr, handler).await?;
    finish_connect(opts, handle, discovered).await
}

async fn connect_stream<S>(
    opts: ConnectOpts,
    stream: S,
    _bastion: Option<Established>,
) -> Result<Established>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (handler, discovered) = FerraHandler::new();
    let config = client_config(&opts)?;
    let handle = client::connect_stream(Arc::new(config), stream, handler).await?;
    finish_connect(opts, handle, discovered).await
}

fn client_config(opts: &ConnectOpts) -> Result<client::Config> {
    let preferred = opts.algs.to_preferred()?;
    Ok(client::Config {
        inactivity_timeout: None,
        keepalive_interval: (opts.keepalive > 0).then(|| Duration::from_secs(opts.keepalive)),
        keepalive_max: 5,
        preferred,
        nodelay: true,
        ..Default::default()
    })
}

async fn finish_connect(
    opts: ConnectOpts,
    mut handle: Handle<FerraHandler>,
    discovered: Arc<Mutex<Option<(String, String)>>>,
) -> Result<Established> {
    let found = discovered.lock().await.clone();
    let (fingerprint, algorithm) = found.ok_or_else(|| Error::msg("server did not present a host key"))?;
    let reject = if let Some(expected) = &opts.expected_fingerprint {
        expected != &fingerprint && !opts.accept_unknown_host
    } else {
        !opts.accept_unknown_host
    };
    if reject {
        let err = if let Some(expected) = opts.expected_fingerprint.clone() {
            Error::HostKeyMismatch {
                host: opts.host.clone(),
                port: opts.port,
                expected,
                actual: fingerprint,
            }
        } else {
            Error::UnknownHostKey {
                host: opts.host.clone(),
                port: opts.port,
                fingerprint,
            }
        };
        let _ = handle.disconnect(Disconnect::ByApplication, "host key not trusted", "en").await;
        return Err(err);
    }
    authenticate(&mut handle, &opts).await?;
    Ok(Established { handle: Arc::new(Mutex::new(handle)), fingerprint, algorithm })
}

async fn authenticate(handle: &mut Handle<FerraHandler>, opts: &ConnectOpts) -> Result<()> {
    match opts.auth {
        AuthMethodKind::Password | AuthMethodKind::Keyboard => {
            if opts.auth == AuthMethodKind::Keyboard || opts.secret.password.is_some() {
                if let Some(password) = &opts.secret.password {
                    if handle.authenticate_password(&opts.username, password).await?.success() {
                        return Ok(());
                    }
                }
                keyboard_interactive(handle, opts).await
            } else {
                Err(Error::AuthFailed)
            }
        }
        AuthMethodKind::Key => {
            let pem = opts
                .private_key_pem
                .as_deref()
                .or(opts.secret.private_key_pem.as_deref())
                .ok_or_else(|| Error::msg("no private key provided"))?;
            let passphrase = opts.secret.passphrase.as_deref();
            let key = decode_key(pem, passphrase)?;
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let auth = handle
                .authenticate_publickey(&opts.username, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if auth.success() {
                Ok(())
            } else {
                Err(Error::AuthFailed)
            }
        }
        AuthMethodKind::Agent => authenticate_agent(handle, opts).await,
    }
}

async fn keyboard_interactive(handle: &mut Handle<FerraHandler>, opts: &ConnectOpts) -> Result<()> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(&opts.username, None::<String>)
        .await?;
    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => return Ok(()),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Err(Error::AuthFailed),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts
                    .iter()
                    .map(|p| {
                        if p.prompt.to_ascii_lowercase().contains("password") {
                            opts.secret.password.clone().unwrap_or_default()
                        } else if p.echo {
                            opts.username.clone()
                        } else {
                            opts.secret.password.clone().unwrap_or_default()
                        }
                    })
                    .collect();
                response = handle.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
}

async fn authenticate_agent(handle: &mut Handle<FerraHandler>, opts: &ConnectOpts) -> Result<()> {
    #[cfg(windows)]
    {
        let mut agent = russh::keys::agent::client::AgentClient::connect_pageant().await;
        return agent_try(handle, opts, &mut agent).await;
    }
    #[cfg(unix)]
    {
        let mut agent = russh::keys::agent::client::AgentClient::connect_env()
            .await
            .map_err(|e| Error::msg(format!("SSH agent: {e}")))?;
        return agent_try(handle, opts, &mut agent).await;
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (handle, opts);
        Err(Error::msg("SSH agent is not available on this platform"))
    }
}

async fn agent_try<S>(
    handle: &mut Handle<FerraHandler>,
    opts: &ConnectOpts,
    agent: &mut russh::keys::agent::client::AgentClient<S>,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| Error::msg(format!("agent identities: {e}")))?;
    if identities.is_empty() {
        return Err(Error::msg("SSH agent has no identities (start Pageant / ssh-agent)"));
    }
    let hash = handle.best_supported_rsa_hash().await?.flatten();
    for key in identities {
        match handle
            .authenticate_publickey_with(&opts.username, key, hash, agent)
            .await
        {
            Ok(res) if res.success() => return Ok(()),
            Ok(_) => continue,
            Err(e) => tracing::debug!("agent key rejected: {e}"),
        }
    }
    Err(Error::AuthFailed)
}

pub async fn start_local_forward(
    handle: Arc<Mutex<Handle<FerraHandler>>>,
    bind: std::net::SocketAddr,
    dest_host: String,
    dest_port: u16,
) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|e| Error::msg(format!("listen {bind}: {e}")))?;
    tokio::spawn(async move {
        loop {
            let Ok((mut incoming, _)) = listener.accept().await else { break };
            let handle = handle.clone();
            let dest_host = dest_host.clone();
            tokio::spawn(async move {
                let channel = {
                    let h = handle.lock().await;
                    h.channel_open_direct_tcpip(&dest_host, dest_port as u32, "127.0.0.1", 0)
                        .await
                };
                let Ok(channel) = channel else { return };
                let mut remote = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut incoming, &mut remote).await;
            });
        }
    });
    Ok(())
}

pub fn decode_key(pem: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
    decode_secret_key(pem, passphrase).map_err(|e| Error::msg(format!("cannot parse private key: {e}")))
}

pub async fn open_shell(
    handle: &Arc<Mutex<Handle<FerraHandler>>>,
    term: &str,
    cols: u32,
    rows: u32,
) -> Result<Channel<client::Msg>> {
    let h = handle.lock().await;
    let channel = h.channel_open_session().await?;
    channel.request_pty(false, term, cols, rows, 0, 0, &[]).await?;
    channel.request_shell(true).await?;
    Ok(channel)
}

pub async fn open_sftp_channel(
    handle: &Arc<Mutex<Handle<FerraHandler>>>,
) -> Result<russh_sftp::client::SftpSession> {
    let h = handle.lock().await;
    let channel = h.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    Ok(russh_sftp::client::SftpSession::new(channel.into_stream()).await?)
}

pub async fn disconnect(handle: &Arc<Mutex<Handle<FerraHandler>>>) -> Result<()> {
    handle.lock().await.disconnect(Disconnect::ByApplication, "bye", "en").await?;
    Ok(())
}

pub async fn exec_capture(handle: &Arc<Mutex<Handle<FerraHandler>>>, command: &str) -> Result<String> {
    let mut channel = {
        let h = handle.lock().await;
        h.channel_open_session().await?
    };
    channel.exec(true, command).await?;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let collect = async {
        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { ref data } => stdout.extend_from_slice(data),
                ChannelMsg::ExtendedData { ref data, .. } => stderr.extend_from_slice(data),
                ChannelMsg::Eof | ChannelMsg::Close => break,
                ChannelMsg::ExitStatus { .. } => {}
                _ => {}
            }
        }
        String::from_utf8_lossy(&stdout).into_owned()
    };
    match tokio::time::timeout(std::time::Duration::from_secs(12), collect).await {
        Ok(text) => {
            if text.trim().is_empty() && !stderr.is_empty() {
                return Err(Error::msg(String::from_utf8_lossy(&stderr)));
            }
            Ok(text)
        }
        Err(_) => Err(Error::msg("remote command timed out")),
    }
}

pub fn known_from(host: &str, port: u16, established: &Established) -> KnownHost {
    KnownHost {
        host: host.into(),
        port,
        algorithm: established.algorithm.clone(),
        fingerprint: established.fingerprint.clone(),
    }
}

pub async fn pump_channel(mut channel: Channel<client::Msg>, mut on_data: impl FnMut(&[u8])) -> Result<Option<u32>> {
    let mut code = None;
    while let Some(msg) = channel.wait().await {
        match msg {
            ChannelMsg::Data { ref data } => on_data(data),
            ChannelMsg::ExtendedData { ref data, .. } => on_data(data),
            ChannelMsg::ExitStatus { exit_status } => {
                code = Some(exit_status);
                let _ = channel.eof().await;
                break;
            }
            ChannelMsg::Eof | ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok(code)
}
