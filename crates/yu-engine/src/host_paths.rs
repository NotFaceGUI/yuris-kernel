//! 宿主路径注入(平台注入口)。
//!
//! 手册 `basic/svariable.html` 的四个路径系统变量由**宿主**给值,内核不猜:
//! `$_SYSTEM_PATH` / `$_SAVE_PATH` / `$_APPDATA_PATH` / `$_PERSONAL_PATH`
//! (`$_PROJECT_PATH` 手册无页,DIV-RT-010,保持 `None`)。
//!
//! # 为什么要端口化
//!
//! 旧实现直接读 `USERPROFILE`,等于把「Windows 桌面」写死进装配层:
//! - Linux / macOS 没有 `USERPROFILE` ⇒ 注入为空,脚本拼路径的支路失效;
//! - Android 应用的「主目录 / 个人目录」不在环境变量里,必须由宿主按
//!   `Context.getFilesDir()` 之类给出;读环境变量只会得到空或 `/`。
//!
//! 现在分两层:平台默认解析([`PlatformHome::resolve`]) + 宿主显式覆盖
//! ([`HostPathOverrides`])。有真实文件系统的平台(桌面 / Android)因此都能
//! 拿到**可写、可拼接**的路径,游戏侧脚本看到的语义与官方一致。
//!
//! # 分隔符
//!
//! 官方是 win32 程序,脚本里的路径拼接以 `\` 为分隔符,但也接受 `/`
//! (Windows API 两者等价)。本模块输出**正斜杠**形式(跨平台字符串拼接
//! 安全),仅在拼「个人目录」子目录时沿用作系统分隔符,保持与既有 Windows
//! 输出逐字节一致的描述。

use std::path::{Path, PathBuf};

/// 宿主要覆盖的路径系统变量(未给的项走平台默认)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostPathOverrides {
    /// `$_SYSTEM_PATH`(游戏起動パス;默认 = 存档根的父目录)。
    pub system_path: Option<PathBuf>,
    /// `$_APPDATA_PATH`(用户主目录;默认见 [`PlatformHome`])。
    pub appdata_path: Option<PathBuf>,
    /// `$_PERSONAL_PATH`(マイドキュメント;默认 = 主目录下 `Documents`)。
    pub personal_path: Option<PathBuf>,
    /// `$_PROJECT_PATH`(手册无页;默认 `None`,不猜)。
    pub project_path: Option<Vec<u8>>,
}

/// 平台默认主目录。
///
/// 只读环境变量,不探测文件系统;拿不到就是 `None`(由调用方决定挂号,
/// 不内置兜底目录 —— 那会把「谁都没给」伪装成「有一个可用目录」)。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlatformHome {
    pub home: Option<PathBuf>,
    /// 平台原生的「我的文档」目录(Windows 用 `Documents`,其余用 `Documents`)。
    pub documents: Option<PathBuf>,
}

impl PlatformHome {
    /// 从当前进程环境解析。
    pub fn resolve() -> Self {
        let home = home_dir();
        let documents = home.as_ref().map(|h| h.join(DOCUMENTS_DIR));
        Self { home, documents }
    }

    /// 显式给值(宿主已知自己的目录时用,例如 Android 的 app 私有目录)。
    pub fn explicit(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let documents = home.join(DOCUMENTS_DIR);
        Self {
            home: Some(home),
            documents: Some(documents),
        }
    }
}

/// 「我的文档」在用户主目录下的标准名。
///
/// 官方引擎在 Windows 用 `Documents`(实测 `USERPROFILE\Documents`);
/// 其余平台同名即可,脚本只把它当路径前缀拼接,不参与格式判定。
const DOCUMENTS_DIR: &str = "Documents";

/// 用户主目录(平台变量优先顺序)。
fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(value) = non_empty_var("USERPROFILE") {
            return Some(PathBuf::from(value));
        }
        // 少数受限环境(< Windows 2000 风格 / 精简服务账户)只有 HOMEDRIVE+HOMEPATH。
        if let (Some(drive), Some(path)) =
            (non_empty_var("HOMEDRIVE"), non_empty_var("HOMEPATH"))
        {
            return Some(PathBuf::from(format!("{drive}{path}")));
        }
        None
    }
    #[cfg(not(windows))]
    {
        non_empty_var("HOME").map(PathBuf::from)
    }
}

fn non_empty_var(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// 把路径转成脚本可见的字节形式(正斜杠、目录后缀 `/`)。
fn script_dir(path: &Path) -> Vec<u8> {
    let mut text = path.to_string_lossy().replace('\\', "/");
    if !text.ends_with('/') {
        text.push('/');
    }
    text.into_bytes()
}

/// 构造 `host_paths` 的最终值。
///
/// 优先级:宿主显式覆盖 > 平台默认 > 由存档根推导。`$_SAVE_PATH` 恒为
/// 存档根(调用方已决定重定向),`$_SYSTEM_PATH` 缺省为其父目录。
pub fn resolve_host_paths(
    save_root: &Path,
    overrides: &HostPathOverrides,
    platform: &PlatformHome,
) -> yu_rt::vm::HostPaths {
    let system_root = overrides.system_path.as_deref().unwrap_or_else(|| {
        save_root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(save_root)
    });
    let appdata = overrides
        .appdata_path
        .clone()
        .or_else(|| platform.home.clone());
    let personal = overrides
        .personal_path
        .clone()
        .or_else(|| platform.documents.clone());
    yu_rt::vm::HostPaths {
        system_path: Some(script_dir(system_root)),
        save_path: Some(script_dir(save_root)),
        appdata_path: appdata.as_deref().map(script_dir),
        personal_path: personal.as_deref().map(script_dir),
        project_path: overrides.project_path.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_path_is_normalized_with_trailing_slash() {
        let paths = resolve_host_paths(
            Path::new("proj/save"),
            &HostPathOverrides::default(),
            &PlatformHome::explicit("/home/u"),
        );
        assert_eq!(paths.save_path.as_deref(), Some(b"proj/save/".as_slice()));
        assert_eq!(paths.system_path.as_deref(), Some(b"proj/".as_slice()));
        assert_eq!(paths.appdata_path.as_deref(), Some(b"/home/u/".as_slice()));
        assert_eq!(
            paths.personal_path.as_deref(),
            Some(b"/home/u/Documents/".as_slice())
        );
        assert_eq!(paths.project_path, None, "手册无页 ⇒ 不猜");
    }

    #[test]
    fn overrides_win_over_platform_defaults() {
        let overrides = HostPathOverrides {
            system_path: Some(PathBuf::from("/game")),
            appdata_path: Some(PathBuf::from("/data/app")),
            personal_path: Some(PathBuf::from("/sdcard/Documents")),
            project_path: Some(b"/game/project".to_vec()),
        };
        let paths = resolve_host_paths(
            Path::new("/data/app/save"),
            &overrides,
            &PlatformHome::explicit("/home/u"),
        );
        assert_eq!(paths.system_path.as_deref(), Some(b"/game/".as_slice()));
        assert_eq!(paths.save_path.as_deref(), Some(b"/data/app/save/".as_slice()));
        assert_eq!(paths.appdata_path.as_deref(), Some(b"/data/app/".as_slice()));
        assert_eq!(
            paths.personal_path.as_deref(),
            Some(b"/sdcard/Documents/".as_slice())
        );
        assert_eq!(paths.project_path.as_deref(), Some(b"/game/project".as_slice()));
    }

    #[test]
    fn absolute_save_root_keeps_its_own_parent_as_system_path() {
        let paths = resolve_host_paths(
            Path::new("/opt/game/save"),
            &HostPathOverrides::default(),
            &PlatformHome::default(),
        );
        assert_eq!(paths.system_path.as_deref(), Some(b"/opt/game/".as_slice()));
        assert_eq!(paths.save_path.as_deref(), Some(b"/opt/game/save/".as_slice()));
        assert_eq!(paths.appdata_path, None, "没有主目录就不编一个出来");
        assert_eq!(paths.personal_path, None);
    }
}
