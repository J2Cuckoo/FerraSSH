import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";

const COMPANY = "贵州力贤网络科技有限公司";
const PRODUCT = "FerraSSH";
const HOMEPAGE = "https://hyrubik.com/#/products/ferrassh";
const YEAR = new Date().getFullYear();

type Props = { onClose: () => void };

async function open(url: string) {
  try {
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

export default function AboutPage({ onClose }: Props) {
  const [version, setVersion] = useState(__APP_VERSION__);

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(__APP_VERSION__));
  }, []);

  return (
    <div className="about-page">
      <div className="about-inner">
        <header className="about-hero">
          <span className="logo">Fs</span>
          <div className="about-hero-text">
            <h1>{PRODUCT}</h1>
            <p>原生 SSH / SFTP 工作站</p>
            <p className="about-ver">
              v{version} · 构建 {__BUILD_DATE__}
            </p>
          </div>
          <button type="button" onClick={onClose}>
            返回
          </button>
        </header>

        <section>
          <h2>软件基本信息</h2>
          <dl>
            <dt>软件名称</dt>
            <dd>{PRODUCT}</dd>
            <dt>软件作者</dt>
            <dd>{COMPANY}</dd>
            <dt>版本号</dt>
            <dd>v{version}</dd>
            <dt>构建日期</dt>
            <dd>{__BUILD_DATE__}</dd>
            <dt>产品形态</dt>
            <dd>主密码保护的本机 SSH / SFTP 工作站</dd>
          </dl>
        </section>

        <section>
          <h2>版权与法律声明</h2>
          <dl>
            <dt>版权所有</dt>
            <dd>
              © {YEAR} {COMPANY}
            </dd>
            <dt>许可证</dt>
            <dd>MIT License</dd>
            <dt>声明</dt>
            <dd>
              本软件按现状提供，不附带任何明示或默示担保。凭据仅保存在本机加密库中，发行方不对远程主机、网络路径或第三方服务承担责任。
            </dd>
          </dl>
        </section>

        <section>
          <h2>开发者 / 发行方</h2>
          <dl>
            <dt>开发商</dt>
            <dd>{COMPANY}</dd>
            <dt>产品主页</dt>
            <dd>
              <button type="button" className="link" onClick={() => void open(HOMEPAGE)}>
                {HOMEPAGE}
              </button>
            </dd>
          </dl>
        </section>

        <section>
          <h2>系统与环境</h2>
          <dl>
            <dt>操作系统</dt>
            <dd>Windows 10 及以上 · macOS 12 及以上 · 主流 64 位 Linux</dd>
            <dt>最低配置</dt>
            <dd>64 位处理器，2 GB 内存，200 MB 可用磁盘，可用网络连接</dd>
            <dt>建议配置</dt>
            <dd>4 GB 及以上内存，固态硬盘</dd>
            <dt>公司信息</dt>
            <dd>{COMPANY}</dd>
          </dl>
        </section>
      </div>
    </div>
  );
}
