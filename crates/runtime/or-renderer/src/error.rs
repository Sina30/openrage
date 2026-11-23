use thiserror::Error;
#[derive(Error, Debug)]
pub enum RenderError {
    #[error("No suitable GPU adapter found: {0}")]
    AdapterNotFound(String),
    #[error("Failed to create surface: {0}")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),
    #[error("Failed to request device: {0}")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),
    #[error("Surface unavailable: {0}")]
    SurfaceUnavailable(String),
    #[error("Window handle error: {0}")]
    WindowHandle(String),
}
