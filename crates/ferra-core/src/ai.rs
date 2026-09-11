//! LLM catalog, hidden ops-engineer prompt, command safety, JSON knowledge base.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const DONE_MARK: &str = "__FERRA_AI_DONE__";

pub fn system_prompt() -> &'static str {
    r#"你是 SSH 运维助手。必须简洁：禁止啰嗦、绕远、复述大段终端输出或重复已知事实。
规则：
1. 只做可验证操作。未知先查服务器，禁止臆造路径、服务、版本或结果。
2. 每步只做一件事：一条命令、等待用户、或结束。没有步数上限，问题没真正解决就禁止结束。
3. say 最多两句，只说本步要点。不要解释原理，不要列备选方案。
4. 危险命令（删除、改核心配置、重启、关机、病毒相关等）不会被直接执行，系统会先征求用户同意。不要绕过，不要拆成「看起来无害」的连环命令去规避。
5. 知识库匹配当前主机时必须优先遵循。
6. 命令是一行真实 shell，不要 markdown，不要一次多条。
7. 优先直接解决用户提出的问题本身：先核对用户输入是否规范、是否写错路径/服务名/配置。禁止为了「显得专业」而安装一堆无关软件或插件。确需临时测试工具时，问题解决后必须删除该工具及所有测试产生的非必要文件。
8. 需要用户在本机操作（上传文件、改权限、确认风险、在终端输入）时：command 为 null，done 为 false，wait 为 upload / permission / user，wait_path 为可选远端路径。系统会循环检测：用户发消息、点继续、或终端回到空闲提示符后会通知你。收到通知后必须立刻按原计划继续，不要再问「是否已完成」。
9. wget、curl、apt、yum、dnf、pip、npm、git clone、docker pull、scp、rsync 等下载或拉取命令：系统会等到下载真正结束并回到提示符后才把结果给你。禁止用 &、nohup、disown 把下载丢到后台，禁止没等到结果就规划下一步。
10. 若用户拒绝某条危险命令，把它当作否定答案，立刻改用更安全的方案，不要重复同一命令。
11. 集群任务时 JSON 可带 node（节点会话 id）。先综合各节点日志再定位到具体节点，再对该节点下命令。
12. 只输出 JSON，不要其它文字：
{"say":"一句说明","command":"命令或null","done":false,"wait":null,"wait_path":null,"node":null}
仅当问题已验证解决、或确认无法继续时：done true，command null，wait null。done 为 false 且 command 为空视为等待用户，不会结束任务。"#
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub models: Vec<AiModel>,
    pub needs_key: bool,
    pub hint: String,
}

/// Curated for SSH ops: text chat / coder models only (no image, video, ASR, TTS).
/// Snapshot ~2026-08; users can type any model id from the vendor console.
pub fn providers() -> Vec<AiProvider> {
    vec![
        p(
            "deepseek",
            "DeepSeek",
            "https://api.deepseek.com",
            &[
                ("deepseek-v4-flash", "DeepSeek V4 Flash"),
                ("deepseek-v4-pro", "DeepSeek V4 Pro"),
            ],
            true,
            "国内常用，OpenAI 兼容。Pro 适合复杂排障，Flash 适合多轮命令循环",
        ),
        p(
            "qwen",
            "阿里云百炼",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            &[
                ("qwen3-coder-plus", "Qwen3 Coder Plus"),
                ("qwen3.7-max", "Qwen3.7 Max"),
                ("qwen3.7-plus", "Qwen3.7 Plus"),
                ("qwen3.7-flash", "Qwen3.7 Flash"),
            ],
            true,
            "旧 dashscope 域名仍可用。业务空间专属域名可自行填入接口地址",
        ),
        p(
            "anthropic",
            "Anthropic Claude",
            "https://api.anthropic.com",
            &[
                ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
                ("claude-opus-4-8", "Claude Opus 4.8"),
                ("claude-fable-5", "Claude Fable 5"),
                ("claude-haiku-4-5", "Claude Haiku 4.5"),
            ],
            true,
            "指令遵循强，适合逐步排障。鉴权走 x-api-key",
        ),
        p(
            "openai",
            "OpenAI",
            "https://api.openai.com/v1",
            &[
                ("gpt-5.6-sol", "GPT-5.6 Sol"),
                ("gpt-5.6-terra", "GPT-5.6 Terra"),
                ("gpt-5.6-luna", "GPT-5.6 Luna"),
            ],
            true,
            "Sol 适合复杂推理与脚本，Terra 均衡，Luna 低成本",
        ),
        p(
            "zhipu",
            "智谱 GLM",
            "https://open.bigmodel.cn/api/paas/v4",
            &[
                ("glm-5.1", "GLM-5.1"),
                ("glm-5", "GLM-5"),
                ("glm-4.7", "GLM-4.7"),
            ],
            true,
            "中文运维与长程任务。Coding 网关可自行改接口地址",
        ),
        p(
            "moonshot",
            "Moonshot Kimi",
            "https://api.moonshot.cn/v1",
            &[
                ("kimi-k2-turbo-preview", "Kimi K2 Turbo"),
                ("moonshot-v1-128k", "Moonshot 128k"),
            ],
            true,
            "长上下文，适合贴大段日志与配置",
        ),
        p(
            "volcengine",
            "火山方舟",
            "https://ark.cn-beijing.volces.com/api/v3",
            &[
                ("doubao-seed-2-1-pro-260628", "Doubao Seed 2.1 Pro"),
                ("doubao-seed-2-0-code-preview", "Doubao Seed 2.0 Code"),
                ("doubao-seed-2-0-pro", "Doubao Seed 2.0 Pro"),
            ],
            true,
            "模型名可改为控制台接入点 ID。Coding 地址可自行替换",
        ),
        p(
            "hunyuan",
            "腾讯混元",
            "https://api.hunyuan.cloud.tencent.com/v1",
            &[
                ("hunyuan-code", "混元 Code"),
                ("hunyuan-turbo-latest", "混元 Turbo"),
                ("hunyuan-large", "混元 Large"),
                ("hunyuan-pro", "混元 Pro"),
            ],
            true,
            "OpenAI 兼容网关。Code 更适合脚本与排障",
        ),
        p(
            "baidu",
            "百度千帆",
            "https://qianfan.baidubce.com/v2",
            &[
                ("ernie-4.5-turbo-128k", "ERNIE 4.5 Turbo"),
                ("ernie-4.5-8k", "ERNIE 4.5"),
                ("ernie-4.0-8k", "ERNIE 4.0"),
            ],
            true,
            "统一 /v2/chat/completions，用模型名切换",
        ),
        p(
            "gemini",
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            &[
                ("gemini-3.1-pro-preview", "Gemini 3.1 Pro"),
                ("gemini-3.5-flash", "Gemini 3.5 Flash"),
                ("gemini-3.1-flash-lite", "Gemini 3.1 Flash-Lite"),
            ],
            true,
            "OpenAI 兼容端点。3.1 Pro 偏软件工程与多步工具",
        ),
        p(
            "xai",
            "xAI Grok",
            "https://api.x.ai/v1",
            &[("grok-4.6", "Grok 4.6"), ("grok-4", "Grok 4")],
            true,
            "OpenAI 兼容，适合编码与长程任务",
        ),
        p(
            "groq",
            "Groq",
            "https://api.groq.com/openai/v1",
            &[
                ("llama-3.3-70b-versatile", "Llama 3.3 70B"),
                ("llama-3.1-8b-instant", "Llama 3.1 8B Instant"),
            ],
            true,
            "低延迟，OpenAI 兼容",
        ),
        p(
            "openrouter",
            "OpenRouter",
            "https://openrouter.ai/api/v1",
            &[
                ("anthropic/claude-sonnet-4.6", "Claude Sonnet 4.6"),
                ("openai/gpt-5.6-sol", "GPT-5.6 Sol"),
                ("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro"),
                ("qwen/qwen3-coder-plus", "Qwen3 Coder Plus"),
            ],
            true,
            "聚合多家，一套 Key 可换模型",
        ),
        p(
            "ollama",
            "Ollama 本地",
            "http://127.0.0.1:11434/v1",
            &[
                ("qwen2.5-coder", "Qwen 2.5 Coder"),
                ("qwen2.5", "Qwen 2.5"),
                ("deepseek-r1", "DeepSeek R1"),
                ("llama3.1", "Llama 3.1"),
            ],
            false,
            "本机 Ollama，接口地址必填，Key 可不填。模型名需与 ollama list 一致",
        ),
    ]
}

fn p(id: &str, name: &str, base: &str, models: &[(&str, &str)], needs_key: bool, hint: &str) -> AiProvider {
    AiProvider {
        id: id.into(),
        name: name.into(),
        base_url: base.into(),
        models: models
            .iter()
            .map(|(mid, label)| AiModel {
                id: (*mid).into(),
                name: (*label).into(),
            })
            .collect(),
        needs_key,
        hint: hint.into(),
    }
}

fn default_provider() -> String {
    "deepseek".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_danger_commands")]
    pub danger_commands: Vec<String>,
}

pub fn default_danger_commands() -> Vec<String> {
    vec![
        "rm -rf".into(),
        "rm -fr".into(),
        "rm --no-preserve-root".into(),
        "mkfs".into(),
        "wipefs".into(),
        "dd if=".into(),
        "shutdown".into(),
        "reboot".into(),
        "poweroff".into(),
        "halt".into(),
        "systemctl restart".into(),
        "systemctl stop".into(),
        "systemctl reboot".into(),
        "init 0".into(),
        "init 6".into(),
        "kill -9".into(),
        "chmod -R".into(),
        "chown -R".into(),
        "visudo".into(),
        "/etc/passwd".into(),
        "/etc/shadow".into(),
        "/etc/ssh/sshd_config".into(),
        "iptables -F".into(),
        "ufw disable".into(),
        "virus".into(),
        "ransomware".into(),
        "mkfs.".into(),
    ]
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: default_provider(),
            model: "deepseek-v4-flash".into(),
            api_key: String::new(),
            base_url: "https://api.deepseek.com".into(),
            danger_commands: default_danger_commands(),
        }
    }
}

impl AiSettings {
    pub fn resolve_base_url(&self) -> String {
        let custom = self.base_url.trim();
        if !custom.is_empty() {
            return custom.trim_end_matches('/').to_string();
        }
        providers()
            .into_iter()
            .find(|p| p.id == self.provider)
            .map(|p| p.base_url)
            .unwrap_or_else(|| "https://api.deepseek.com".into())
    }

    pub fn needs_key(&self) -> bool {
        providers()
            .iter()
            .find(|p| p.id == self.provider)
            .map(|p| p.needs_key)
            .unwrap_or(true)
    }

    pub fn validate_for_enable(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if self.provider.trim().is_empty() {
            return Err(Error::msg("请选择大模型平台"));
        }
        if self.model.trim().is_empty() {
            return Err(Error::msg("请选择或填写模型名称"));
        }
        if self.base_url.trim().is_empty() && self.provider == "ollama" {
            return Err(Error::msg("使用 Ollama 时必须填写本机接口地址"));
        }
        if self.needs_key() && self.api_key.trim().is_empty() {
            return Err(Error::msg("开启 AI 前必须填写 API Key"));
        }
        Ok(())
    }

    pub fn sanitized(&self) -> Self {
        let mut s = self.clone();
        s.base_url = s.base_url.trim().trim_end_matches('/').to_string();
        s
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbSourceSummary {
    pub source: String,
    pub count: i64,
    pub titles: Vec<String>,
}

pub fn kb_template_json() -> &'static str {
    r#"{
  "knowledge": [
    {
      "title": "Nginx 反向代理",
      "tags": ["nginx", "proxy", "web"],
      "content": "适用于公网 Web 入口。安装 nginx 后在 conf.d 增加 server，listen 80；location / 使用 proxy_pass 指向上游，并设置 Host、X-Real-IP、X-Forwarded-For、X-Forwarded-Proto。先 nginx -t 再 reload，不要直接覆盖主配置而不备份。"
    },
    {
      "title": "内网仅 SSH 加固",
      "tags": ["ssh", "intranet", "security"],
      "content": "内网主机优先禁用密码登录、仅允许密钥；PermitRootLogin prohibit-password；更改默认端口需同步放行防火墙。不要对公网 22 端口做无限制暴露。修改 sshd_config 后必须先开一个已验证会话再重启 sshd。"
    },
    {
      "title": "防火墙放行 Web",
      "tags": ["firewalld", "ufw", "80", "443"],
      "content": "公网机器需要 80/443。firewalld: firewall-cmd --permanent --add-service=http --add-service=https && firewall-cmd --reload。ufw: ufw allow 80/tcp && ufw allow 443/tcp。内网仅跳板访问时不要对 0.0.0.0 开放数据库端口。"
    }
  ]
}
"#
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KbDoc {
    pub id: String,
    pub source: String,
    pub title: String,
    pub body: String,
    pub tags: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LlmStep {
    #[serde(default)]
    pub say: String,
    pub command: Option<String>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub wait: Option<String>,
    #[serde(default)]
    pub wait_path: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
}

pub fn command_is_dangerous(cmd: &str) -> bool {
    command_matches_danger(cmd, &default_danger_commands())
}

pub fn command_matches_danger(cmd: &str, extra: &[String]) -> bool {
    let lower = cmd.to_lowercase();
    if lower.contains("mkfs.")
        || lower.contains("mkfs ")
        || lower.contains("--no-preserve-root")
        || lower.contains(":(){")
        || lower.contains("fork bomb")
        || lower.contains("of=/dev/sd")
        || lower.contains("of=/dev/nvme")
        || lower.contains("of=/dev/mmc")
        || lower.contains("> /dev/sd")
        || lower.contains(">/dev/sd")
        || lower.contains("wipefs")
        || lower.contains("chmod -r 777 /")
        || lower.contains("chmod -r 000 /")
    {
        return true;
    }
    for part in lower.split(|c| c == ';' || c == '&' || c == '|' || c == '\n') {
        let toks: Vec<&str> = part.split_whitespace().collect();
        if toks.iter().any(|t| *t == "rm" || t.ends_with("/rm")) {
            for tok in &toks {
                if *tok == "/" || *tok == "/*" || *tok == "/." || *tok == "--no-preserve-root" {
                    return true;
                }
            }
        }
    }
    extra.iter().any(|p| {
        let pat = p.trim().to_lowercase();
        !pat.is_empty() && lower.contains(&pat)
    })
}

pub fn parse_llm_json(raw: &str) -> Result<LlmStep> {
    let t = raw.trim();
    let json = if let Some(start) = t.find('{') {
        let end = t.rfind('}').ok_or_else(|| Error::msg("模型未返回 JSON"))?;
        &t[start..=end]
    } else {
        return Err(Error::msg("模型未返回 JSON"));
    };
    Ok(serde_json::from_str(json)?)
}

pub fn parse_kb_json(source: &str, raw: &str) -> Result<Vec<(String, String, String)>> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|_| Error::msg("知识库必须是合法 JSON"))?;
    let mut out = Vec::new();
    collect_docs(&v, source, &mut out);
    if out.is_empty() {
        return Err(Error::msg("JSON 中没有可用条目（需要 title/content 或字符串数组）"));
    }
    Ok(out)
}

fn collect_docs(v: &serde_json::Value, source: &str, out: &mut Vec<(String, String, String)>) {
    match v {
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_docs(item, source, out);
            }
        }
        serde_json::Value::Object(map) => {
            for key in ["documents", "items", "knowledge", "entries", "data"] {
                if let Some(inner) = map.get(key) {
                    collect_docs(inner, source, out);
                    if !out.is_empty() {
                        return;
                    }
                }
            }
            let title = map
                .get("title")
                .or_else(|| map.get("name"))
                .or_else(|| map.get("id"))
                .and_then(|x| x.as_str())
                .unwrap_or(source)
                .to_string();
            let body = map
                .get("content")
                .or_else(|| map.get("body"))
                .or_else(|| map.get("text"))
                .or_else(|| map.get("value"))
                .or_else(|| map.get("desc"))
                .or_else(|| map.get("description"));
            let tags = map
                .get("tags")
                .map(|t| match t {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Array(a) => a
                        .iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                    _ => String::new(),
                })
                .unwrap_or_default();
            if let Some(b) = body {
                let text = match b {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !text.trim().is_empty() {
                    out.push((title, text, tags));
                    return;
                }
            }
            let rest = serde_json::to_string_pretty(map).unwrap_or_default();
            if rest.len() > 8 {
                out.push((title, rest, tags));
            }
        }
        serde_json::Value::String(s) if !s.trim().is_empty() => {
            out.push((source.into(), s.clone(), String::new()));
        }
        _ => {}
    }
}

pub fn is_private_ip(ip: &str) -> bool {
    let ip = ip.split('/').next().unwrap_or(ip).trim();
    if ip.starts_with("10.") || ip.starts_with("192.168.") || ip.starts_with("127.") || ip.starts_with("169.254.") {
        return true;
    }
    if let Some(rest) = ip.strip_prefix("172.") {
        if let Some((a, _)) = rest.split_once('.') {
            if let Ok(n) = a.parse::<u8>() {
                return (16..=31).contains(&n);
            }
        }
    }
    false
}

pub fn network_scope(nics: &[crate::monitor::NicMetric]) -> String {
    let mut private = false;
    let mut public = false;
    for n in nics {
        for part in n.ipv4.split_whitespace() {
            let addr = part.split('/').next().unwrap_or(part);
            if addr.is_empty() || addr.starts_with("127.") {
                continue;
            }
            if is_private_ip(addr) {
                private = true;
            } else {
                public = true;
            }
        }
    }
    match (private, public) {
        (true, true) => "同时具备内网与公网地址".into(),
        (true, false) => "主要为内网环境".into(),
        (false, true) => "主要为公网环境".into(),
        _ => "网络范围未知".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_rm_root() {
        assert!(command_is_dangerous("rm -rf /"));
        assert!(command_is_dangerous("rm -rf /*"));
        assert!(command_is_dangerous("sudo rm -fr / "));
        assert!(command_is_dangerous("systemctl restart nginx"));
        assert!(!command_is_dangerous("ls /var/cache/nginx"));
    }

    #[test]
    fn parses_kb_array() {
        let docs = parse_kb_json("a.json", r#"[{"title":"nginx","content":"listen 80"}]"#).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].0, "nginx");
    }

    #[test]
    fn parses_llm_fence() {
        let s = parse_llm_json("```json\n{\"say\":\"ok\",\"command\":null,\"done\":true}\n```").unwrap();
        assert!(s.done);
        assert!(s.command.is_none());
        let w = parse_llm_json(r#"{"say":"请上传配置","command":null,"done":false,"wait":"upload","wait_path":"/etc/nginx/nginx.conf"}"#).unwrap();
        assert_eq!(w.wait.as_deref(), Some("upload"));
        assert_eq!(w.wait_path.as_deref(), Some("/etc/nginx/nginx.conf"));
    }
}
