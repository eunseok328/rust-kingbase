//! Kingbase compatibility helpers.

// ═══════════════════ [新增开始] Kingbase 四模式兼容模块入口 ═══════════════════
// Keep Kingbase compatibility code behind a small internal module so the
// original PostgreSQL path can stay as the common fallback.
pub(crate) mod mysql_types;
pub mod types;
// ═══════════════════ [新增结束] Kingbase 四模式兼容模块入口 ═══════════════════
