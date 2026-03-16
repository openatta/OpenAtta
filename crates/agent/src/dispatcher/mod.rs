//! Tool call dispatcher abstraction
//!
//! Provides [`ToolDispatcher`] trait with two implementations:
//! - [`NativeToolDispatcher`] — pass-through for models with native tool calling
//! - [`XmlToolDispatcher`] — XML-based tool calling for models without native support

pub mod native;
pub mod traits;
pub mod xml;

pub use native::NativeToolDispatcher;
pub use traits::{DispatchResult, ToolDispatcher};
pub use xml::XmlToolDispatcher;

use std::sync::Arc;

use crate::llm::ModelInfo;

/// Select the appropriate dispatcher based on model capabilities
pub fn select_dispatcher(model_info: &ModelInfo) -> Arc<dyn ToolDispatcher> {
    if model_info.supports_tools {
        Arc::new(NativeToolDispatcher)
    } else {
        Arc::new(XmlToolDispatcher::new())
    }
}
