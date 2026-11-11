//! 法律声明页。
//!
//! 对照 `pointer-official/apps/web/src/app/legal/page.tsx` 移植：引言 + 五节正文，
//! 文案与官网逐字一致。首页底部的《法律声明》链接指向这里。

use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;

use crate::state::Shared;

pub fn router() -> Router<Shared> {
    Router::new().route("/legal", get(index))
}

async fn index() -> Response {
    Html(LEGAL).into_response()
}

const LEGAL: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>法律声明 | Pointer</title>
<meta name="description" content="Pointer 平台法律声明与合规使用边界说明。">
<link rel="icon" type="image/png" href="/app-icon.png">
<style>
*{box-sizing:border-box}
body{margin:0;background:#fff;color:#18181B;
font-family:-apple-system,BlinkMacSystemFont,"Segoe UI","PingFang SC","Microsoft YaHei",sans-serif;
-webkit-font-smoothing:antialiased}
a{color:inherit;text-decoration:none}
header{position:relative;z-index:20;height:56px;display:flex;align-items:center;
justify-content:space-between;padding:0 20px;border-bottom:1px solid rgba(0,0,0,.06);
background:rgba(255,255,255,.92);backdrop-filter:blur(8px)}
.brand{display:flex;align-items:center;gap:9px;font-weight:600;font-size:15px}
.brand img{width:24px;height:24px;border-radius:6px;display:block}
nav{display:flex;align-items:center;gap:22px;font-size:14px;color:rgba(24,24,27,.72)}
nav a:hover{color:#18181B}

main{max-width:1024px;margin:0 auto;padding:64px 16px;display:flex;flex-direction:column;gap:32px}
.eyebrow{margin:0 0 16px;font-size:14px;font-weight:500;letter-spacing:.12em;color:#0B6CF0}
h1{margin:0 0 16px;font-size:2.5rem;font-weight:600;letter-spacing:-.02em;line-height:1.15}
.intro{margin:0;max-width:760px;font-size:1.0625rem;line-height:1.75;color:rgba(0,0,0,.7)}

section{padding:24px;border-radius:16px;border:1px solid rgba(0,0,0,.05);background:#fff;
box-shadow:0 1px 3px rgba(0,0,0,.06)}
section h2{margin:0 0 12px;font-size:1.5rem;font-weight:600;letter-spacing:-.02em}
section p{margin:0 0 12px;font-size:15px;line-height:1.85;color:rgba(0,0,0,.7)}
section p:last-child{margin-bottom:0}
</style>
</head>
<body>
<header>
  <a class="brand" href="/"><img src="/app-icon.png" alt="Pointer" width="24" height="24">Pointer</a>
  <nav>
    <a href="/download">下载</a>
    <a href="/login">登录</a>
  </nav>
</header>

<main>
  <div>
    <p class="eyebrow">LEGAL</p>
    <h1>法律声明</h1>
    <p class="intro">
      Pointer 为计算机智能体与桌面自动化工具平台。用户在使用本平台及相关功能时，应遵守中华人民共和国法律法规、监管要求、
      网络服务规则以及所访问目标网站、应用、接口与设备的使用协议。
    </p>
  </div>

  <section>
    <h2>一、合法合规使用原则</h2>
    <p>用户仅可在具有合法权限、明确授权和正当业务目的的前提下使用 Pointer 的自动化能力。</p>
    <p>平台能力适用于个人助理、企业内部作业流转、测试验证、运营辅助、客服处理、数据录入等合法场景。</p>
    <p>
      用户不得利用本平台从事任何违法违规活动，包括但不限于侵入他人系统、绕过访问控制、批量注册、恶意营销、诈骗、刷量、盗取数据、
      干扰平台正常运营或其他损害第三方合法权益的行为。
    </p>
  </section>

  <section>
    <h2>二、关于验证码与人机校验功能的特别说明</h2>
    <p>
      平台在部分自动化流程中，可能涉及对验证码、短信校验码、图形校验、人机挑战或类似验证步骤的识别、填写、提交或协助完成。
    </p>
    <p>
      上述能力仅限用户在合法、合规且经目标系统授权的业务流程中使用，例如用户本人账号登录、企业自有系统操作、获得委托的流程代办、
      内部测试环境验证等正当场景。
    </p>
    <p>
      用户不得将上述能力用于绕过、规避、破坏或削弱第三方平台的安全验证、人机识别、风险控制、访问限制、反作弊或反滥用机制；不得用于批量化、
      对抗式、伪装式或其他明显违背目标平台规则的用途。
    </p>
    <p>
      如目标网站、应用或服务明确禁止自动化操作、验证码代处理或类似行为，用户应立即停止使用相关能力。因用户违反目标平台规则或适用法律法规产生的责任，
      由用户自行承担。
    </p>
  </section>

  <section>
    <h2>三、用户责任与保证</h2>
    <p>用户应确保其输入、任务指令、登录凭证、业务数据及自动化目标均来源合法，并已取得必要授权。</p>
    <p>用户应自行判断其自动化任务是否符合所在行业监管要求、劳动用工规范、数据安全义务与隐私保护要求。</p>
    <p>
      对于用户通过平台访问、处理、传输或生成的任何数据、内容或操作结果，用户应自行承担相应法律责任，并负责保存必要的授权、留痕与审计记录。
    </p>
  </section>

  <section>
    <h2>四、平台权利与限制措施</h2>
    <p>
      如平台发现或有合理理由怀疑用户存在违法违规、超出授权范围、规避安全机制、滥用验证码处理能力或其他高风险行为，平台有权视情况采取限制功能、
      中止服务、终止账户、保留日志、配合监管或司法机关调查等措施。
    </p>
    <p>平台有权根据法律法规、监管政策、业务安全需要及风险治理要求，随时调整相关功能范围、使用限制与审核机制。</p>
  </section>

  <section>
    <h2>五、免责与联系方式</h2>
    <p>
      平台不承诺任何第三方网站、应用、接口或业务系统始终支持自动化访问，也不对第三方系统规则变更、风控升级、验证码策略调整、网络故障或不可抗力导致的
      任务失败承担责任。
    </p>
    <p>若您对本法律声明或平台合规使用边界有疑问，请联系运营主体：深圳元智体人工智能有限公司。</p>
  </section>
</main>
</body>
</html>"#;
