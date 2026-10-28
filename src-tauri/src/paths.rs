// 路径解析 —— 全部收敛到这一个文件（Rust 侧权威实现）
// 数据目录策略(按优先级):
// 1. YUNTING_DATA_DIR 环境变量 —— 显式覆盖(单测/便携自选目录)
// 2. %APPDATA%\yunting —— 标准位置(与 Electron 版数据隔离, 避免双开冲突)
// 3. exe 旁 data/      —— 仅当 %APPDATA% 不可写时兜底(便携场景)
// 内核目录: 打包态 exe 旁 kernel/; 开发态项目 kernel/
use std::path::{Path, PathBuf};

pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// 目录真实可写探测（create_dir_all 对已存在目录不验证可写性，必须实测）
fn is_writable_dir(dir: &Path) -> bool {
    let probe = dir.join(".write-probe");
    match std::fs::write(&probe, b"x") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// 解析数据目录（无缓存纯函数，测试直接测这个）
/// 1. YUNTING_DATA_DIR 环境变量（显式覆盖）
/// 2. %APPDATA%\yunting —— 必须真实可写才采用（受限会话/ACL 异常时 create_dir_all 会假成功）
/// 3. exe 旁 data/
pub fn resolve_data_dir() -> PathBuf {
    resolve_data_dir_with(
        std::env::var("YUNTING_DATA_DIR").ok(),
        std::env::var("APPDATA").ok(),
        &exe_dir(),
    )
}

/// 可注入参数的纯逻辑（便于测试，不依赖进程全局环境变量）
pub fn resolve_data_dir_with(
    override_dir: Option<String>,
    appdata: Option<String>,
    exe_dir: &Path,
) -> PathBuf {
    if let Some(o) = override_dir {
        if !o.trim().is_empty() {
            let d = PathBuf::from(o);
            let _ = std::fs::create_dir_all(&d);
            return d;
        }
    }
    if let Some(ad) = appdata {
        let dir = Path::new(&ad).join("yunting");
        if std::fs::create_dir_all(&dir).is_ok() && dir.is_dir() && is_writable_dir(&dir) {
            return dir;
        }
    }
    // 兜底: exe 旁 data/
    let d = exe_dir.join("data");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn data_dir() -> PathBuf {
    use std::sync::OnceLock;
    static CACHE: OnceLock<PathBuf> = OnceLock::new();
    CACHE.get_or_init(resolve_data_dir).clone()
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
    //   target\debug\yunting.exe → 项目根；target\debug\deps\test.exe → 项目根
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
