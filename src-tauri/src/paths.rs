// 路径解析 —— 全部收敛到这一个文件（Rust 侧权威实现）
// 数据目录策略(按优先级):
// 1. %APPDATA%\yt-tool —— 标准位置(与 Electron 版数据隔离, 避免双开冲突)
// 2. exe 旁 data/      —— 仅当 %APPDATA% 不可写时兜底(便携场景)
// 内核目录: 打包态 exe 旁 kernel/; 开发态项目 kernel/
use std::path::{Path, PathBuf};

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// 是否打包态: exe 名为 yt-tool / 云亭（非 target\debug\yt-tool.exe 形态）
/// 开发态路径形如 ...\yt-tool-tauri\src-tauri\target\debug\yt-tool.exe
fn is_packaged() -> bool {
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy().to_lowercase();
    // cargo 构建产物在 target\ 目录下 → 开发/构建调试态
    !(s.contains("target\\debug\\") || s.contains("target\\release\\") || s.contains("/target/debug/") || s.contains("/target/release/"))
}

pub fn data_dir() -> PathBuf {
    use std::sync::OnceLock;
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(|| {
        if let Ok(ad) = std::env::var("APPDATA") {
            let dir = Path::new(&ad).join("yt-tool");
            if std::fs::create_dir_all(&dir).is_ok() && dir.is_dir() {
                return dir;
            }
        }
        // 兜底: exe 旁 data/
        let d = exe_dir().join("data");
        let _ = std::fs::create_dir_all(&d);
        d
    })
    .clone()
}

pub fn kernel_dir() -> PathBuf {
    // 打包态: exe 旁 kernel/
    {
        let k = exe_dir().join("kernel");
        if k.join("sing-box.exe").exists() {
            return k;
        }
    }
    // 开发态: 从 exe 向上找包含 kernel/sing-box.exe 的目录
    //   target\debug\yt-tool.exe → 项目根；target\debug\deps\test.exe → 项目根
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().skip(1) {
            let k = anc.join("kernel");
            if k.join("sing-box.exe").exists() {
                return k;
            }
        }
    }
    // 兜底
    exe_dir().join("kernel")
}

pub fn kernel_exe() -> PathBuf {
    kernel_dir().join("sing-box.exe")
}

/// 用户手动把 data 放 exe 旁时（便携模式识别）:  exe 旁存在 data/ 目录
#[allow(dead_code)]
pub fn is_portable_hint() -> bool {
    exe_dir().join("data").is_dir()
}
