use writ_core::Error;

use crate::ui::AppState;

pub fn render(_state: &AppState) -> Result<String, Error> {
    Ok(r#"<div class="shell">Health is clear.</div>"#.into())
}
