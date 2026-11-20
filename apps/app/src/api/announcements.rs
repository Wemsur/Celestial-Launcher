//! Thin commands for the launcher's built-in announcement feed.

use crate::api::Result;
use std::sync::Arc;
use tauri::{Emitter, plugin::TauriPlugin};
use theseus::{State, announcements::{self, AnnouncementBody, AnnouncementPage, Snapshot}};

pub fn init<R: tauri::Runtime>() -> TauriPlugin<R> {
	tauri::plugin::Builder::new("announcements")
		.invoke_handler(tauri::generate_handler![
			announcement_snapshot,
			announcement_list,
			announcement_get,
			announcement_refresh,
			announcement_suppress,
		])
		.build()
}

fn state() -> Result<Arc<State>> {
	Ok(State::get_if_initialized().ok_or_else(|| {
		theseus::Error::from(theseus::ErrorKind::InputError("Announcement storage is not ready".into()))
	})?)
}

#[tauri::command]
pub async fn announcement_snapshot() -> Result<Snapshot> {
	let state = state()?;
	Ok(announcements::snapshot(&state).await?)
}

#[tauri::command]
pub async fn announcement_list(cursor: Option<usize>, limit: usize) -> Result<AnnouncementPage> {
	let state = state()?;
	Ok(announcements::list(&state, cursor, limit).await?)
}

#[tauri::command]
pub async fn announcement_get(id: String) -> Result<AnnouncementBody> {
	let state = state()?;
	Ok(announcements::get(&state, &id).await?)
}

#[tauri::command]
pub async fn announcement_suppress(id: String) -> Result<()> {
	let state = state()?;
	announcements::suppress(&state, &id).await?;
	Ok(())
}

#[tauri::command]
pub async fn announcement_refresh<R: tauri::Runtime>(app: tauri::AppHandle<R>) -> Result<bool> {
	let state = state()?;
	if !announcements::begin_refresh() {
		return Ok(false);
	}
	let _ = app.emit("announcements-updated", ());
	let _task = tauri::async_runtime::spawn(async move {
		let _ = announcements::refresh(&state).await;
		let _ = app.emit("announcements-updated", ());
	});
	Ok(true)
}
