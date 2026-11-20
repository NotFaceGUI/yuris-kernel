//! 存档写盘的**版本档案**:一个容器版本对应的全部格式策略。
//!
//! 存档容器的版本号(`YSSD +0x04`)是**写盘器常量**,不是 yscfg/引擎版本
//! (DIV-RT-058)。同一版本可能有多项随版本变化的写盘策略:
//!
//! | 策略 | 字段 | 证据 |
//! |---|---|---|
//! | 图像块描述符布局 | [`SaveProfile::image_layout`] | 0.479 官方 LOAD 硬校验 A=0x600、w/h@+0x70(`DIV-RT-062`) |
//! | 块类型 0 的 var_id | [`SaveProfile::btype0_var_id`] | 0.479 官方写 0;0.495 写实际 id(真实存档逐块) |
//! | 块压缩层 | 不在此表 | 由盘上 `.sd`/引擎 `YSSNP` 学习(独立于版本) |
//!
//! **加新版本 = 在 [`profile_for`] 加一行 + 在 [`tests`] 加断言**。写盘端
//! 只消费本表;未列出的版本按“未取证”处理:**不猜图像块布局**,变量块
//! 照常写盘并挂 `sv.image.layout`。
//!
//! 读盘端不受本表限制:A 段长度是版本指纹,`w*h*4 == B` 自证
//! (见 `image_block::decode`);本表只决定**写**。

use crate::image_block::{self, Layout};
use yu_rt::scriptpos::{ScriptPosWire, WIRE_GOSUB_U16, WIRE_GOSUB_U8};

/// 块类型 0 的 `var_id` 写盘策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btype0VarId {
    /// 写 0(0.479 官方产物:所有 btype 0 块的 var_id 均为 0)。
    Zero,
    /// 写实际变量 id(0.495/0.481/0.474 系列;真实存档逐块对照)。
    Actual,
}

/// 一个已取证容器版本的完整写盘策略。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveProfile {
    /// 图像块(btype 1)描述符布局。
    ///
    /// `None` = 该版本已知但图像块布局未取证:写盘时跳过缩略图块并挂号,
    /// 不写一个官方读不了的布局。
    pub image_layout: Option<Layout>,
    /// 块类型 0 的 var_id 策略。
    pub btype0_var_id: Btype0VarId,
    /// `TASKINFO[SCRIPTPOS]` 的线上格式(任务块里的执行位串)。
    ///
    /// 与容器版本同源:0.450–0.479(0.13) 的官方装载器把 **GOSUB 计数
    /// 读成 u16**(4 字符),0.480 起读 u8(2 字符);两代都在 `text_line`
    /// 前带 1 字节尾部 u8。逐版机器码:
    /// - u16:0.450 `0x440515`、0.466 `0x4420bd`、0.470 `0x4428e9`、
    ///   0.474 `0x448f69`、0.479 `0x449f3d/0x449f58`(`add ecx/ebp,4`);
    /// - u8:0.480 `0x44fa3a` 一带、0.495 `0x4511cc`、v494(`add ebp,2`)。
    /// 写错宽度 → 官方读档时脚本名整体错位(报「2\script\...」)。
    pub scriptpos: ScriptPosWire,
}

/// 按容器版本取写盘档案;`None` = 完全没有该版本的证据。
///
/// 版本来源优先级见 `SaveState::flush`(官方 exe 常量 → 盘上学习 →
/// 目标文件自身 → 压缩层默认)。本表只回答“拿到版本后怎么写”。
#[must_use]
pub fn profile_for(version: u32) -> Option<SaveProfile> {
    match version {
        // 0.450–0.466 全系(21 个 SDK 包逐一下载分析):官方 LOAD
        // 版本常量 0xC0,硬校验 A=0x5F0(0.459 `FUN_004382d0` /
        // 0.466 `FUN_00438b78`),w/h 从描述符 +0x68 取。
        // 块类型 0 的 var_id 没有官方存档样本(LOAD 不消费该字段),
        // 暂按实际 id 写;拿到 0.45x/0.46x 存档后补断言。
        0x00C0 => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0466),
            btype0_var_id: Btype0VarId::Actual,
            scriptpos: WIRE_GOSUB_U16,
        }),
        // 0.470–0.479(0.13):官方 LOAD 版本常量 0x138、A=0x600、
        // w/h 在 +0x70(0.479 `FUN_00440490` 0x440894;0.47x 全系
        // exe 同一常量)。官方 save00001 DNO2 为标量、btype0 var_id=0。
        0x0138 => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0479),
            btype0_var_id: Btype0VarId::Zero,
            scriptpos: WIRE_GOSUB_U16,
        }),
        // 0.479(0.14)–0.495 全系:官方 LOAD 版本常量 0x1E0、
        // A=0x640、w/h 在 +0x78(0.495 exe 0x44734e/0x4477de;
        // 0.48x/0.49x 全系 exe 同一常量;0.495 官方存档样本逐块实测)。
        0x01E0 => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0495),
            btype0_var_id: Btype0VarId::Actual,
            scriptpos: WIRE_GOSUB_U8,
        }),
        // 0.481 商业构建(非 SDK)写 0x1E1;布局与 0x1E0 同(Erewhon
        // 官方存档 A=1600/+0x78)。
        0x01E1 => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0495),
            btype0_var_id: Btype0VarId::Actual,
            scriptpos: WIRE_GOSUB_U8,
        }),
        // 商业 0.49x 的变体(光翼戦姫样本);布局同 0x1E0 族。
        0x01E6 => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0495),
            btype0_var_id: Btype0VarId::Actual,
            scriptpos: WIRE_GOSUB_U8,
        }),
        // 以下两项**不是官方容器版本**,是旧内核把工程 yscfg 版本误当
        // 容器版本写出的历史产物(DIV-RT-058)。保留只为读旧档/覆写兼容:
        // 官方 LOAD 会在版本检查处拒收它们,新盘不要主动选。
        0x01DA | 0x01EF => Some(SaveProfile {
            image_layout: Some(image_block::LAYOUT_0495),
            btype0_var_id: Btype0VarId::Actual,
            scriptpos: WIRE_GOSUB_U8,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_block::{LAYOUT_0466, LAYOUT_0479, LAYOUT_0495};

    /// 这张表是写盘版本策略的唯一来源:每个已取证版本都必须显式断言,
    /// 防止后续改动把某一代悄悄写错(加到新版本时在这里补一行)。
    #[test]
    fn observed_containers_have_expected_profiles() {
        let cases = [
            (
                0x00C0,
                Some(LAYOUT_0466),
                Btype0VarId::Actual,
                WIRE_GOSUB_U16,
            ),
            (0x0138, Some(LAYOUT_0479), Btype0VarId::Zero, WIRE_GOSUB_U16),
            (
                0x01DA,
                Some(LAYOUT_0495),
                Btype0VarId::Actual,
                WIRE_GOSUB_U8,
            ),
            (
                0x01E0,
                Some(LAYOUT_0495),
                Btype0VarId::Actual,
                WIRE_GOSUB_U8,
            ),
            (
                0x01E1,
                Some(LAYOUT_0495),
                Btype0VarId::Actual,
                WIRE_GOSUB_U8,
            ),
            (
                0x01E6,
                Some(LAYOUT_0495),
                Btype0VarId::Actual,
                WIRE_GOSUB_U8,
            ),
            (
                0x01EF,
                Some(LAYOUT_0495),
                Btype0VarId::Actual,
                WIRE_GOSUB_U8,
            ),
        ];
        for (version, layout, var_id, scriptpos) in cases {
            let profile = profile_for(version).unwrap_or_else(|| panic!("{version:#x} 无档案"));
            assert_eq!(profile.image_layout, layout, "{version:#x} 图像块布局");
            assert_eq!(profile.btype0_var_id, var_id, "{version:#x} var_id 策略");
            assert_eq!(
                profile.scriptpos, scriptpos,
                "{version:#x} SCRIPTPOS 线格式"
            );
        }
    }

    #[test]
    fn unknown_versions_have_no_profile_and_never_invent_a_layout() {
        for version in [0x0139, 0x01DB, 0x01FF, 0x0555] {
            assert!(profile_for(version).is_none(), "{version:#x} 不应有档案");
        }
    }

    /// 图像布局的公开查询走同一张表(避免两处各写一份 match)。
    #[test]
    fn image_layout_query_delegates_to_profile() {
        for version in [0x00C0, 0x0138, 0x01DA, 0x01E0, 0x01E1, 0x01E6, 0x01EF] {
            assert_eq!(
                image_block::layout_for_container_version(version),
                profile_for(version).and_then(|p| p.image_layout),
                "{version:#x}"
            );
        }
        assert_eq!(image_block::layout_for_container_version(0x0555), None);
    }
}
