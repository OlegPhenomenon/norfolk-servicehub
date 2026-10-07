//! Axum multipart with the shared JSON validation envelope.
use crate::{
    error::{AppError, AppResult},
    state::AppState,
};
use axum::extract::{FromRequest, Request};
pub struct Multipart(axum::extract::Multipart);
impl FromRequest<AppState> for Multipart {
    type Rejection = AppError;
    async fn from_request(req: Request, state: &AppState) -> AppResult<Self> {
        axum::extract::Multipart::from_request(req, state)
            .await
            .map(Self)
            .map_err(|_| AppError::field("file", "Send a valid multipart file upload."))
    }
}
impl std::ops::Deref for Multipart {
    type Target = axum::extract::Multipart;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for Multipart {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
