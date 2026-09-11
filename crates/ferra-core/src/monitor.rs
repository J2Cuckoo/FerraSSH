//! Remote host metrics collected over a non-PTY SSH exec channel.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const COLLECT_SH: &str = r#"
export LC_ALL=C
echo HOST	$(hostname 2>/dev/null || echo unknown)
echo UNAME	$(uname -srm 2>/dev/null)
echo KERNEL	$(uname -r 2>/dev/null)
echo ARCH	$(uname -m 2>/dev/null)
if [ -r /etc/os-release ]; then
  OS_NAME=$(awk -F= '/^PRETTY_NAME=/{gsub(/"/,"",$2); print $2; exit}' /etc/os-release)
  echo OS	${OS_NAME:-unknown}
elif [ -r /etc/redhat-release ]; then
  echo OS	$(cat /etc/redhat-release)
else
  echo OS	$(uname -s 2>/dev/null)
fi
echo CPU_MODEL	$(awk -F: '/^model name/{gsub(/^ +/,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null)
if command -v systemd-detect-virt >/dev/null 2>&1; then
  echo VIRT	$(systemd-detect-virt 2>/dev/null || echo unknown)
elif [ -r /sys/class/dmi/id/product_name ]; then
  echo VIRT	$(cat /sys/class/dmi/id/product_name 2>/dev/null)
else
  echo VIRT	unknown
fi
echo UPTIME	$(awk '{print int($1)}' /proc/uptime 2>/dev/null)
echo LOAD	$(awk '{print $1,$2,$3}' /proc/loadavg 2>/dev/null)
echo CORES	$(nproc 2>/dev/null || grep -c '^processor' /proc/cpuinfo 2>/dev/null || echo 1)
c1=$(awk '/^cpu /{idle=$5; tot=0; for(i=2;i<=NF;i++) tot+=$i; print idle, tot}' /proc/stat 2>/dev/null)
sleep 1
c2=$(awk '/^cpu /{idle=$5; tot=0; for(i=2;i<=NF;i++) tot+=$i; print idle, tot}' /proc/stat 2>/dev/null)
echo CPU	$c1	$c2
awk '/^MemTotal:/{print "MEM_TOTAL",$2}
     /^MemAvailable:/{print "MEM_AVAIL",$2}
     /^MemFree:/{print "MEM_FREE",$2}
     /^Buffers:/{print "MEM_BUF",$2}
     /^Cached:/{print "MEM_CACHE",$2}
     /^SwapTotal:/{print "SWAP_TOTAL",$2}
     /^SwapFree:/{print "SWAP_FREE",$2}' /proc/meminfo 2>/dev/null
df -kP 2>/dev/null | awk 'NR>1 && $1 !~ /^(tmpfs|devtmpfs|overlay|squashfs|udev)$/ && $6 ~ /^\// {print "DISK",$6,$1,$2,$3,$5}'
awk -F'[: ]+' 'NR>2 {gsub(/ /,"",$1); print "NET",$1,$2,$3,$5,$10,$11,$13}' /proc/net/dev 2>/dev/null
for n in /sys/class/net/*; do
  [ -d "$n" ] || continue
  name=$(basename "$n")
  state=$(cat "$n/operstate" 2>/dev/null)
  mac=$(cat "$n/address" 2>/dev/null)
  mtu=$(cat "$n/mtu" 2>/dev/null)
  ipv4=""
  if command -v ip >/dev/null 2>&1; then
    ipv4=$(ip -o -4 addr show dev "$name" 2>/dev/null | awk '{print $4}' | tr '\n' ' ')
  fi
  echo NIC	$name	${state:-unknown}	${mac:-}	${mtu:-0}	${ipv4:-}
done
fw_emit() { echo FW	"$1"	"$2"	"$3"	"$4"; }
if command -v firewall-cmd >/dev/null 2>&1 || command -v firewalld >/dev/null 2>&1 || [ -e /usr/lib/systemd/system/firewalld.service ]; then
  if pidof firewalld >/dev/null 2>&1 || systemctl is-active --quiet firewalld 2>/dev/null; then
    fw_emit firewalld yes on running
  else
    fw_emit firewalld yes off stopped
  fi
else
  fw_emit firewalld no off
fi
if command -v ufw >/dev/null 2>&1; then
  ust=$(ufw status 2>/dev/null | awk 'NR==1{print tolower($2); exit}')
  if [ "$ust" = "active" ]; then fw_emit ufw yes on active; else fw_emit ufw yes off "${ust:-inactive}"; fi
else
  fw_emit ufw no off
fi
if command -v nft >/dev/null 2>&1; then
  nt=$(nft list tables 2>/dev/null | wc -l | tr -d ' ')
  if [ "${nt:-0}" -gt 0 ]; then fw_emit nft yes on "${nt}tables"; else fw_emit nft yes off empty; fi
else
  fw_emit nft no off
fi
if [ -r /proc/net/ip_tables_names ]; then
  names=$(tr '\n' ',' < /proc/net/ip_tables_names | sed 's/,$//')
  if [ -n "$names" ]; then fw_emit iptables yes on "$names"; else fw_emit iptables yes off; fi
elif command -v iptables >/dev/null 2>&1; then
  fw_emit iptables yes off present
else
  fw_emit iptables no off
fi
who 2>/dev/null | head -n 20 | while IFS= read -r line; do
  [ -n "$line" ] && echo WHO	"$line"
done
"#;

const LOGIN_HISTORY_SH: &str = r#"
export LC_ALL=C
last -n 30 2>/dev/null | head -n 30 | while IFS= read -r line; do
  case "$line" in
    ""|wtmp*|reboot*|shutdown*) continue ;;
  esac
  echo LAST	"$line"
done
"#;

const AUTH_LOG_SH: &str = r#"
export LC_ALL=C
authlog=""
[ -r /var/log/secure ] && authlog=/var/log/secure
[ -r /var/log/auth.log ] && authlog=/var/log/auth.log
if [ -n "$authlog" ]; then
  tail -n 400 "$authlog" 2>/dev/null | grep -E 'sshd.*(Accepted|Failed|Invalid|Disconnected|session opened|session closed)' | tail -n 30 | while IFS= read -r line; do
    echo AUTH	"$line"
  done
fi
"#;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiskMetric {
    pub mount: String,
    pub source: String,
    pub total_kb: u64,
    pub used_kb: u64,
    pub percent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetMetric {
    pub iface: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    #[serde(default)]
    pub rx_packets: u64,
    #[serde(default)]
    pub tx_packets: u64,
    #[serde(default)]
    pub rx_drop: u64,
    #[serde(default)]
    pub tx_drop: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NicMetric {
    pub name: String,
    pub state: String,
    pub mac: String,
    pub mtu: u32,
    pub ipv4: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FirewallMetric {
    pub name: String,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostMetrics {
    pub hostname: String,
    pub uname: String,
    #[serde(default)]
    pub os_name: String,
    #[serde(default)]
    pub kernel: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub cpu_model: String,
    #[serde(default)]
    pub virt: String,
    pub uptime_secs: u64,
    pub cpu_percent: f32,
    pub cpu_cores: u32,
    pub load1: f32,
    pub load5: f32,
    pub load15: f32,
    pub mem_total_kb: u64,
    pub mem_available_kb: u64,
    pub mem_used_kb: u64,
    pub swap_total_kb: u64,
    pub swap_used_kb: u64,
    pub disks: Vec<DiskMetric>,
    pub nets: Vec<NetMetric>,
    pub nics: Vec<NicMetric>,
    pub firewall: Vec<FirewallMetric>,
    #[serde(default)]
    pub sessions: Vec<String>,
}

pub fn collect_script() -> &'static str {
    COLLECT_SH
}

pub fn login_history_script() -> &'static str {
    LOGIN_HISTORY_SH
}

pub fn auth_log_script() -> &'static str {
    AUTH_LOG_SH
}

pub fn parse_tagged_lines(text: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let (key, rest) = split_fields(line);
        if key != tag {
            continue;
        }
        let value = rest.join(" ").trim().to_string();
        if !value.is_empty() {
            out.push(value);
        }
    }
    out
}

pub fn parse_metrics(text: &str) -> Result<HostMetrics> {
    let mut m = HostMetrics::default();
    let mut mem_total = 0u64;
    let mut mem_avail = 0u64;
    let mut mem_free = 0u64;
    let mut mem_buf = 0u64;
    let mut mem_cache = 0u64;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;
    for line in text.lines() {
        let (key, rest) = split_fields(line);
        match key {
            "HOST" => m.hostname = rest.join(" ").trim().into(),
            "UNAME" => m.uname = rest.join(" ").trim().into(),
            "OS" => m.os_name = rest.join(" ").trim().into(),
            "KERNEL" => m.kernel = rest.join(" ").trim().into(),
            "ARCH" => m.arch = rest.join(" ").trim().into(),
            "CPU_MODEL" => m.cpu_model = rest.join(" ").trim().into(),
            "VIRT" => m.virt = rest.join(" ").trim().into(),
            "UPTIME" => m.uptime_secs = rest.first().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
            "LOAD" => {
                let nums: Vec<f32> = rest.join(" ").split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if nums.len() >= 3 {
                    m.load1 = nums[0];
                    m.load5 = nums[1];
                    m.load15 = nums[2];
                }
            }
            "CORES" => m.cpu_cores = rest.first().and_then(|s| s.trim().parse().ok()).unwrap_or(1),
            "CPU" => m.cpu_percent = cpu_from_samples(&rest.join("\t")),
            "MEM_TOTAL" => mem_total = parse_u64(&rest),
            "MEM_AVAIL" => mem_avail = parse_u64(&rest),
            "MEM_FREE" => mem_free = parse_u64(&rest),
            "MEM_BUF" => mem_buf = parse_u64(&rest),
            "MEM_CACHE" => mem_cache = parse_u64(&rest),
            "SWAP_TOTAL" => swap_total = parse_u64(&rest),
            "SWAP_FREE" => swap_free = parse_u64(&rest),
            "DISK" if rest.len() >= 5 => m.disks.push(DiskMetric {
                mount: rest[0].into(),
                source: rest[1].into(),
                total_kb: rest[2].parse().unwrap_or(0),
                used_kb: rest[3].parse().unwrap_or(0),
                percent: rest[4].into(),
            }),
            "NET" if rest.len() >= 3 => m.nets.push(parse_net(&rest)),
            "NIC" if rest.len() >= 4 => m.nics.push(NicMetric {
                name: rest[0].into(),
                state: rest[1].into(),
                mac: rest[2].into(),
                mtu: rest[3].parse().unwrap_or(0),
                ipv4: rest.get(4).copied().unwrap_or("").trim().into(),
            }),
            "FW" if rest.len() >= 2 => m.firewall.push(parse_fw(&rest)),
            "WHO" => {
                let line = rest.join(" ").trim().to_string();
                if !line.is_empty() {
                    m.sessions.push(line);
                }
            }
            _ => {}
        }
    }
    m.mem_total_kb = mem_total;
    m.mem_available_kb = if mem_avail > 0 { mem_avail } else { mem_free + mem_buf + mem_cache };
    m.mem_used_kb = mem_total.saturating_sub(m.mem_available_kb);
    m.swap_total_kb = swap_total;
    m.swap_used_kb = swap_total.saturating_sub(swap_free);
    if m.hostname.is_empty() && m.uname.is_empty() && m.mem_total_kb == 0 {
        return Err(Error::msg("未能采集到主机指标（需要 Linux /proc）"));
    }
    Ok(m)
}

fn split_fields(line: &str) -> (&str, Vec<&str>) {
    if line.contains('\t') {
        let mut parts = line.split('\t');
        (parts.next().unwrap_or(""), parts.collect())
    } else {
        let mut parts = line.split_whitespace();
        (parts.next().unwrap_or(""), parts.collect())
    }
}

fn parse_net(rest: &[&str]) -> NetMetric {
    if rest.len() >= 7 {
        NetMetric {
            iface: rest[0].into(),
            rx_bytes: rest[1].parse().unwrap_or(0),
            rx_packets: rest[2].parse().unwrap_or(0),
            rx_drop: rest[3].parse().unwrap_or(0),
            tx_bytes: rest[4].parse().unwrap_or(0),
            tx_packets: rest[5].parse().unwrap_or(0),
            tx_drop: rest[6].parse().unwrap_or(0),
        }
    } else {
        NetMetric {
            iface: rest[0].into(),
            rx_bytes: rest[1].parse().unwrap_or(0),
            tx_bytes: rest.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
            ..Default::default()
        }
    }
}

fn parse_fw(rest: &[&str]) -> FirewallMetric {
    let name = rest[0].to_string();
    if rest.len() >= 3 && (rest[1] == "yes" || rest[1] == "no") {
        return FirewallMetric {
            name,
            present: rest[1] == "yes",
            active: rest[2] == "on",
            detail: rest.get(3..).map(|s| s.join(" ")).unwrap_or_default(),
        };
    }
    let status = rest[1..].join(" ").to_lowercase();
    let present = !matches!(status.as_str(), "not-installed" | "unknown" | "");
    let active = status.contains("running")
        || status == "active"
        || status == "on"
        || status.contains("tables")
        || (present && !matches!(status.as_str(), "stopped" | "inactive" | "off" | "present"));
    FirewallMetric { name, present, active, detail: rest[1..].join(" ") }
}

fn parse_u64(rest: &[&str]) -> u64 {
    rest.first().and_then(|s| s.trim().parse().ok()).unwrap_or(0)
}

fn cpu_from_samples(s: &str) -> f32 {
    let nums: Vec<f64> = s.split_whitespace().filter_map(|x| x.parse().ok()).collect();
    if nums.len() < 4 {
        return 0.0;
    }
    let (i1, t1, i2, t2) = (nums[0], nums[1], nums[2], nums[3]);
    let di = (i2 - i1).max(0.0);
    let dt = (t2 - t1).max(1.0);
    ((1.0 - di / dt) * 100.0).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sample() {
        let sample = "\
HOST	lab
UNAME	Linux 6.1 x86_64
OS	CentOS Linux 7 (Core)
KERNEL	3.10.0-1160.el7.x86_64
ARCH	x86_64
CPU_MODEL	Intel Xeon
VIRT	kvm
UPTIME	3600
LOAD	0.10 0.20 0.30
CORES	4
CPU	100 200	150 300
MEM_TOTAL	1024000
MEM_AVAIL	512000
SWAP_TOTAL	0
SWAP_FREE	0
DISK	/	/dev/sda1	1000000	400000	40%
NET	eth0	100	10	0	200	8	0
NIC	eth0	up	aa:bb	1500	10.0.0.1/24
FW	firewalld	yes	off	stopped
WHO	root pts/0 10.0.0.8
";
        let m = parse_metrics(sample).unwrap();
        assert_eq!(m.hostname, "lab");
        assert_eq!(m.os_name, "CentOS Linux 7 (Core)");
        assert!((m.cpu_percent - 50.0).abs() < 0.2);
        assert_eq!(m.mem_used_kb, 512000);
        assert_eq!(m.disks[0].mount, "/");
        assert_eq!(m.nics[0].ipv4, "10.0.0.1/24");
        assert_eq!(m.nets[0].rx_packets, 10);
        assert_eq!(m.nets[0].tx_bytes, 200);
        assert!(m.firewall[0].present);
        assert!(!m.firewall[0].active);
        assert_eq!(m.sessions.len(), 1);
        let logs = parse_tagged_lines("LAST	root pts/0 10.0.0.8\nAUTH	sshd accepted\n", "LAST");
        assert_eq!(logs, vec!["root pts/0 10.0.0.8"]);
    }
}
