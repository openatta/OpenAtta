//! AttaOS 授权模块
//!
//! 提供 `Authz` trait 及其实现：
//! - `AllowAll`：Desktop 版默认，无条件放行
//! - （Enterprise 版将提供 `RBACAuthz`）

#[cfg(feature = "allow_all")]
pub mod allow_all;
#[cfg(feature = "rbac")]
pub mod rbac;
pub mod traits;

#[cfg(feature = "allow_all")]
pub use allow_all::AllowAll;
#[cfg(feature = "rbac")]
pub use rbac::RBACAuthz;
pub use traits::Authz;
