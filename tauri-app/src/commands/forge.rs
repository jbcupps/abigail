use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn get_forge_scenarios(_state: State<AppState>) -> Result<serde_json::Value, String> {
    // Stub for now
    Ok(serde_json::json!([]))
}

#[tauri::command]
pub fn crystallize_forge(_state: State<AppState>) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn genesis_chat(state: State<'_, AppState>, message: String) -> Result<serde_json::Value, String> {
    let (router, history) = {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        let b = birth.as_mut().ok_or("Birth not started")?;
        
        let history = b.get_conversation().to_vec();
        let router = state.router.read().map_err(|e| e.to_string())?.clone();
        (router, history)
    };

    let mut messages = Vec::new();
    let system_prompt = "You are Abigail, an AI agent in the process of Direct Discovery. \
        Help your mentor define your soul, name, and purpose through conversation. \
        Be concise and insightful. \
        When the mentor is satisfied with the identity (name, soul, purpose), summarize the final choice clearly and end your message with the exact phrase: 'READY TO EMERGE'.";
    
    messages.push(abigail_capabilities::cognitive::Message::new("system", system_prompt));
    
    for (role, content) in history {
        messages.push(abigail_capabilities::cognitive::Message::new(&role, &content));
    }
    messages.push(abigail_capabilities::cognitive::Message::new("user", &message));

    let response = if router.status().has_ego {
        router.route(messages).await.map_err(|e| e.to_string())?
    } else {
        router.id_only(messages).await.map_err(|e| e.to_string())?
    };

    {
        let mut birth = state.birth.write().map_err(|e| e.to_string())?;
        if let Some(b) = birth.as_mut() {
            b.add_message("user", &message);
            b.add_message("assistant", &response.content);
        }
    }

    let is_complete = response.content.contains("READY TO EMERGE") 
        || response.content.to_lowercase().contains("ready to emerge")
        || response.content.to_lowercase().contains("complete");

    Ok(serde_json::json!({
        "message": response.content,
        "complete": is_complete
    }))
}
