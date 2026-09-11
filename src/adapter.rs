use crate::{Jelto, Props};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime, State,
};

pub trait JeltoExt<R: Runtime> {
    fn jelto(&self) -> Jelto;
}
impl<R: Runtime, T: Manager<R>> JeltoExt<R> for T {
    fn jelto(&self) -> Jelto {
        self.try_state::<Jelto>()
            .map(|state| state.inner().clone())
            .unwrap_or_else(Jelto::unavailable)
    }
}

mod commands {
    use super::*;
    #[tauri::command]
    pub async fn init(
        sdk: State<'_, Jelto>,
        key: String,
        app: Option<String>,
        endpoint: Option<String>,
    ) -> Result<(), ()> {
        sdk.init(&key, app.as_deref(), endpoint.as_deref()).await;
        Ok(())
    }
    #[tauri::command]
    pub async fn track(
        sdk: State<'_, Jelto>,
        name: String,
        props: Option<Props>,
    ) -> Result<(), ()> {
        sdk.track(&name, props).await;
        Ok(())
    }
    #[tauri::command]
    pub async fn onboarding(
        sdk: State<'_, Jelto>,
        step: String,
        status: String,
        reason: Option<String>,
    ) -> Result<(), ()> {
        sdk.onboarding(&step, &status, reason.as_deref()).await;
        Ok(())
    }
    #[tauri::command]
    pub async fn set_props(sdk: State<'_, Jelto>, props: Props) -> Result<(), ()> {
        sdk.set_props(props).await;
        Ok(())
    }
    #[tauri::command]
    pub async fn install_id(sdk: State<'_, Jelto>) -> Result<String, ()> {
        Ok(sdk.install_id().await)
    }
    #[tauri::command]
    pub async fn reset(sdk: State<'_, Jelto>) -> Result<(), ()> {
        sdk.reset().await;
        Ok(())
    }
    #[tauri::command]
    pub async fn disable(sdk: State<'_, Jelto>) -> Result<(), ()> {
        sdk.disable().await;
        Ok(())
    }
}

/// Register inactive. Explicitly initialize after the application's consent decision.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    let mut focused = std::collections::HashSet::new();
    Builder::new("jelto")
        .invoke_handler(tauri::generate_handler![
            commands::init,
            commands::track,
            commands::onboarding,
            commands::set_props,
            commands::install_id,
            commands::reset,
            commands::disable
        ])
        .setup(|app, _| {
            let directory = app
                .path()
                .app_local_data_dir()
                .ok()
                .map(|p| p.join("jelto"));
            app.manage(Jelto::new(
                directory,
                app.package_info().version.to_string(),
            ));
            Ok(())
        })
        .on_event(move |app, event| {
            let flush = match event {
                tauri::RunEvent::ExitRequested { .. } => true,
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::Focused(true),
                    ..
                } => {
                    focused.insert(label.clone());
                    false
                }
                tauri::RunEvent::WindowEvent {
                    label,
                    event: tauri::WindowEvent::Focused(false) | tauri::WindowEvent::Destroyed,
                    ..
                } => {
                    focused.remove(label);
                    focused.is_empty()
                }
                _ => false,
            };
            if flush {
                app.jelto().background();
            }
        })
        .build()
}
