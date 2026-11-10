//! `REGINFO` 的桌面宿主能力:Windows 注册表读取(官方 `0x44bd30` 用
//! `RegOpenKeyExA` + `RegQueryValueExA`,缓冲 1024 字节)。
//!
//! 语义边界:
//! - 子键/值名按官方 A 版 API 的原始字节传入(脚本 CP932),不转 UTF-8;
//! - 只返回官方能读到的值:成功 = `String`(C 串截断)或 `Dword`;
//!   键/值不存在、缓冲不足、非 Windows = `None`(VM 走官方失败默认值);
//! - 不写注册表、不申请额外权限(官方用 `KEY_ALL_ACCESS`,这里同样)。

use yu_rt::event::{RegistryHost, RegistryValue};

/// 桌面宿主注册表读取器。无头/用例不安装 ⇒ VM 使用官方失败默认值。
#[derive(Default)]
pub struct SystemRegistry;

impl RegistryHost for SystemRegistry {
    fn read_value(
        &mut self,
        hive: &str,
        subkey: &[u8],
        name: Option<&[u8]>,
    ) -> Option<RegistryValue> {
        platform::read_value(hive, subkey, name)
    }
}

#[cfg(windows)]
mod platform {
    use super::RegistryValue;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExA, RegQueryValueExA, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_CONFIG,
        HKEY_CURRENT_USER, HKEY_DYN_DATA, HKEY_LOCAL_MACHINE, HKEY_PERFORMANCE_DATA, HKEY_USERS,
        KEY_ALL_ACCESS, REG_DWORD, REG_EXPAND_SZ, REG_SZ,
    };

    fn hive_handle(hive: &str) -> Option<HKEY> {
        Some(match hive {
            "HKEY_CLASSES_ROOT" => HKEY_CLASSES_ROOT,
            "HKEY_CURRENT_USER" => HKEY_CURRENT_USER,
            "HKEY_LOCAL_MACHINE" => HKEY_LOCAL_MACHINE,
            "HKEY_USERS" => HKEY_USERS,
            "HKEY_PERFORMANCE_DATA" => HKEY_PERFORMANCE_DATA,
            "HKEY_CURRENT_CONFIG" => HKEY_CURRENT_CONFIG,
            "HKEY_DYN_DATA" => HKEY_DYN_DATA,
            _ => return None,
        })
    }

    fn c_string(bytes: &[u8]) -> Vec<u8> {
        let mut out = bytes.to_vec();
        if out.last().copied() != Some(0) {
            out.push(0);
        }
        out
    }

    pub(super) fn read_value(
        hive: &str,
        subkey: &[u8],
        name: Option<&[u8]>,
    ) -> Option<RegistryValue> {
        let root = hive_handle(hive)?;
        let subkey = c_string(subkey);
        let name = name.map(c_string);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: 所有指针都指向本栈帧内有效的 NUL 结尾缓冲;句柄只在成功时使用并关闭。
        let status = unsafe {
            RegOpenKeyExA(
                root,
                subkey.as_ptr(),
                0,
                KEY_ALL_ACCESS,
                &mut key as *mut HKEY,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; 1024];
        let mut len = buf.len() as u32;
        let mut ty = 0u32;
        let status = unsafe {
            RegQueryValueExA(
                key,
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                std::ptr::null(),
                &mut ty as *mut u32,
                buf.as_mut_ptr(),
                &mut len as *mut u32,
            )
        };
        // SAFETY: key 来自成功的 RegOpenKeyExA。
        unsafe {
            RegCloseKey(key);
        }
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = (len as usize).min(buf.len());
        let data = &buf[..len];
        match ty {
            REG_SZ | REG_EXPAND_SZ => {
                let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                Some(RegistryValue::String(data[..end].to_vec()))
            }
            REG_DWORD => {
                let mut word = [0u8; 4];
                word[..len.min(4)].copy_from_slice(&data[..len.min(4)]);
                Some(RegistryValue::Dword(u32::from_le_bytes(word)))
            }
            _ => {
                // REG_BINARY/REG_MULTI_SZ 等:官方把原始字节当 C 串交给字符串目标;
                // INT/FLT 目标按处理体不写。这里同样按 C 串截断。
                let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
                Some(RegistryValue::String(data[..end].to_vec()))
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::RegistryValue;

    pub(super) fn read_value(
        _hive: &str,
        _subkey: &[u8],
        _name: Option<&[u8]>,
    ) -> Option<RegistryValue> {
        // 官方是 Windows-only;非 Windows 目标没有注册表 ⇒ 失败默认值。
        None
    }
}
