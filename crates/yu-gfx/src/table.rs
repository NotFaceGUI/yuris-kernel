//! 槽号 → 参数名解析。
//!
//! 铁律:图形域**不得按槽号硬编码**。槽号来自 `VmEvent::DomainCall.args[].slot`,
//! 名字一律查工程 `ysc.ybn`(`yu_rt::tables::YscmTable`,解析自 `yu_format::yscm`)。
//! 跨版本游戏换表即换语义(A5)。

use crate::error;
use yu_rt::event::ArgValue;
use yu_rt::tables::{ParamDecl, YscmTable};

/// 工程命令表投影(只读)。
#[derive(Clone, Debug)]
pub struct ParamTable {
    inner: YscmTable,
}

impl ParamTable {
    pub fn new(inner: YscmTable) -> Self {
        Self { inner }
    }

    pub fn from_tables(tables: &yu_rt::TableSet) -> Self {
        Self::new(tables.boot.yscm.clone())
    }

    pub fn inner(&self) -> &YscmTable {
        &self.inner
    }

    pub fn known(&self, command: &str) -> bool {
        self.inner.index_of(command).is_some()
    }

    pub fn params(&self, command: &str) -> Option<&[ParamDecl]> {
        self.inner
            .index_of(command)
            .and_then(|i| u8::try_from(i).ok())
            .and_then(|i| self.inner.command(i))
            .map(|c| c.params.as_slice())
    }

    /// 槽号 → 参数名。槽号越界/命令不在表里/表内名字为空 = `None`。
    pub fn param_name(&self, command: &str, slot: u8) -> Option<&str> {
        self.params(command)?
            .get(usize::from(slot))
            .map(|p| p.name.as_str())
            .filter(|n| !n.is_empty())
    }

    /// 把事件参数解析成 `(参数名, 槽号, 值)`。名字缺失时保留槽号与 `None`,
    /// 由调用方产出 `gfx.arg.missing` 诊断(不静默猜)。
    pub fn resolve<'a>(&'a self, command: &str, args: &'a [ArgValue]) -> Vec<ResolvedArg<'a>> {
        args.iter()
            .map(|a| ResolvedArg {
                name: self.param_name(command, a.slot),
                slot: a.slot,
                value: &a.value,
                offset: a.offset,
            })
            .collect()
    }

    /// 命令表自身健全性检查:必需图形命令必须存在,命令名必须非空。
    ///
    /// `CG/CGACT/CGEND` 是 `GfxState` 的状态生产入口;缺任一条就说明拿错了
    /// 工程表(或表不完整),必须 fail-fast,而不是运行期逐条静默跳过。
    /// 参数名可以为空:0.495 ERIS 的 `PLUG` 等命令存在空参数槽(引擎表原样),
    /// 这类槽在解析时按“名字缺失”处理,不拒绝整张表。
    pub fn validate(&self) -> Result<(), yu_log::Diag> {
        for required in ["CG", "CGACT", "CGEND"] {
            if !self.known(required) {
                return Err(error::fatal(
                    error::code::TABLE_MISSING,
                    format!("ysc.ybn 缺少图形域必需命令 `{required}`"),
                ));
            }
        }
        for i in 0..self.inner.len() {
            let Some(cmd) = u8::try_from(i).ok().and_then(|i| self.inner.command(i)) else {
                continue;
            };
            if cmd.name.is_empty() {
                return Err(error::fatal(
                    error::code::TABLE_MISSING,
                    format!("ysc.ybn: 命令 {i} 名字为空"),
                ));
            }
        }
        Ok(())
    }
}

/// 解析后的一个参数。
#[derive(Clone, Copy, Debug)]
pub struct ResolvedArg<'a> {
    /// 工程 ysc 表里的参数名;`None` = 槽号越界。
    pub name: Option<&'a str>,
    pub slot: u8,
    pub value: &'a yu_rt::value::Value,
    /// YSCM 参数种类 2(`NumericOffset`,源写法 `X+=`)。只表示数据编码,
    /// **不改变算术语义**(见 `yu_rt::event::ArgValue::offset`)。
    pub offset: bool,
}
