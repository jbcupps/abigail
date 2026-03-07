use std::net::SocketAddr;

use abigail_skills::skill::{ExecutionContext, Skill, ToolOutput, ToolParams};
use axum::{response::Html, routing::get, Router};
use playwright_rs::PLAYWRIGHT_VERSION;
use skill_browser::BrowserSkill;

fn oauth_page() -> Html<&'static str> {
    Html(
        r#"
<!doctype html>
<html>
  <body>
    <script>
      document.cookie = "abigail_session=1; path=/";
      localStorage.setItem("abigail_auth_state", "persisted");
      window.location.assign("/mailbox");
    </script>
  </body>
</html>
"#,
    )
}

fn mailbox_page() -> Html<&'static str> {
    Html(
        r#"
<!doctype html>
<html>
  <body>
    <div id="state"></div>
    <script>
      const cookieReady = document.cookie.includes("abigail_session=1");
      const storageReady = localStorage.getItem("abigail_auth_state") === "persisted";
      document.getElementById("state").textContent = cookieReady && storageReady ? "Logged In" : "Logged Out";
    </script>
  </body>
</html>
"#,
    )
}

async fn spawn_server() -> SocketAddr {
    let app = Router::new()
        .route("/oauth/start", get(|| async { oauth_page() }))
        .route("/mailbox", get(|| async { mailbox_page() }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local test server");
    let address = listener.local_addr().expect("read listener address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test app");
    });
    address
}

fn execution_context() -> ExecutionContext {
    ExecutionContext {
        request_id: "browser-persistent-auth".to_string(),
        user_id: Some("test".to_string()),
    }
}

async fn execute_with_triangle_preview(
    skill: &BrowserSkill,
    tool_name: &str,
    mut params: ToolParams,
) -> Result<ToolOutput, String> {
    let context = execution_context();
    let first = skill
        .execute_tool(tool_name, params.clone(), &context)
        .await
        .map_err(|err| err.to_string())?;
    let data = first
        .data
        .clone()
        .unwrap_or_default();
    if data
        .get("status")
        .and_then(|value| value.as_str())
        == Some("triangle_ethic_preview_required")
    {
        let token = data
            .get("triangle_ethic_preview")
            .and_then(|value| value.get("triangle_ethic_token"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| "missing triangle ethic token".to_string())?;
        params
            .values
            .insert("triangle_ethic_token".to_string(), serde_json::json!(token));
        return skill
            .execute_tool(tool_name, params, &context)
            .await
            .map_err(|err| err.to_string());
    }
    Ok(first)
}

#[tokio::test]
async fn login_survives_browser_restart() {
    let address = spawn_server().await;
    let base_url = format!("http://{address}");
    let temp_dir = tempfile::tempdir().expect("create temp browser profile root");
    let data_dir = temp_dir.path().join("identities").join("entity-123");
    std::fs::create_dir_all(&data_dir).expect("create entity data dir");

    let attempt = async {
        let skill = BrowserSkill::new_for_entity(
            BrowserSkill::default_manifest(),
            true,
            data_dir.clone(),
            Some("entity-123".to_string()),
        );

        execute_with_triangle_preview(
            &skill,
            "login_with_oauth",
            ToolParams::new()
                .with("start_url", format!("{base_url}/oauth/start"))
                .with("success_url_contains", "/mailbox"),
        )
        .await?;

        skill.execute_tool("browser_close", ToolParams::new(), &execution_context())
            .await
            .map_err(|err| err.to_string())?;

        let restarted = BrowserSkill::new_for_entity(
            BrowserSkill::default_manifest(),
            true,
            data_dir.clone(),
            Some("entity-123".to_string()),
        );

        execute_with_triangle_preview(
            &restarted,
            "navigate",
            ToolParams::new().with("url", format!("{base_url}/mailbox")),
        )
        .await?;

        let content = restarted
            .execute_tool(
                "browser_get_content",
                ToolParams::new().with("format", "text"),
                &execution_context(),
            )
            .await
            .map_err(|err| err.to_string())?;
        let page_text = content
            .data
            .as_ref()
            .and_then(|data| data.get("content"))
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if !page_text.contains("Logged In") {
            return Err(format!("expected persisted login, got page text: {page_text}"));
        }

        let session = restarted
            .current_session_record()
            .await
            .ok_or_else(|| "missing browser session record".to_string())?;
        if !session.active_in_process {
            return Err("browser session record should be active after restart".to_string());
        }
        Ok::<(), String>(())
    }
    .await;

    if let Err(err) = attempt {
        let lowered = err.to_ascii_lowercase();
        if lowered.contains("playwright")
            || lowered.contains("install a compatible browser")
            || lowered.contains("browser is not installed")
        {
            eprintln!(
                "Skipping persistent browser auth test because Playwright prerequisites are missing (expected version {}). Error: {}",
                PLAYWRIGHT_VERSION,
                err
            );
            return;
        }
        panic!("{err}");
    }
}
