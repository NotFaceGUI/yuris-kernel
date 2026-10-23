//! 本 crate 的稳定错误码(`fmt.*` 命名空间)。
//!
//! 码是数据:可计数、可进 golden、可直链 `docs/divergences/`。
//! 码表注册在 `yu-log::code`,但该文件归会话 A 所有;本 crate 的码
//! 自持于此,新增码必须同时加入 [`ALL_CODES`]。

/// 容器魔数不符(前 4 字节)。
pub const MAGIC: &str = "fmt.magic";
/// 版本号落在未实现/不支持的形态(如 YSTB V2:无语料,禁臆造)。
pub const VERSION: &str = "fmt.version";
/// 字节流在需要更多数据前结束。
pub const TRUNCATED: &str = "fmt.truncated";
/// 计数与实际可走的记录数不符。
pub const COUNT: &str = "fmt.count";
/// 计数/尺寸超出可表示范围(32 位目标防回绕;不 panic)。
pub const LIMIT: &str = "fmt.limit";
/// 版本落在已知但未实现/未验证的布局带(如 YSTB V2:无语料,禁臆造)。
pub const YBN_UNSUPPORTED: &str = "fmt.ybn.unsupported";
/// YSTB 分段异或校验失败(疑似换键;禁止猜测密钥)。
pub const YBN_KEY: &str = "fmt.ybn.key";
/// YSTB 参数索引区结构损坏(条目数、偏移、长度不自洽)。
pub const YBN_ARGS: &str = "fmt.ybn.args";
/// 指令序号超出 ysc 命令表范围。
pub const YBN_OPCODE: &str = "fmt.ybn.opcode";
/// yscfg 字段未通过引擎侧校验(宽高范围、开关取值、标题长度)。
pub const YSCF_FIELD: &str = "fmt.yscf.field";
/// ysv 记录的种类/类型字节落在已证集合之外。
pub const YSV_FIELD: &str = "fmt.ysv.field";
/// 编码序列非法(含序列不完整)。
pub const CODEC_INVALID: &str = "fmt.codec.invalid";
/// 请求的编解码器不存在。
pub const CODEC_NAME: &str = "fmt.codec.name";
/// YSSD 存档载荷压缩层失败(非 zlib 流/解压不闭合;P4)。
pub const YSSD_CODEC: &str = "fmt.yssd.codec";

/// 全部已登记错误码(顺序稳定)。唯一性由单元测试守卫。
pub const ALL_CODES: &[&str] = &[
    MAGIC,
    VERSION,
    TRUNCATED,
    COUNT,
    LIMIT,
    YBN_UNSUPPORTED,
    YBN_KEY,
    YBN_ARGS,
    YBN_OPCODE,
    YSCF_FIELD,
    YSV_FIELD,
    CODEC_INVALID,
    CODEC_NAME,
    YSSD_CODEC,
];

/// 码是否已登记。用于诊断自检与测试。
pub fn is_registered(code: &str) -> bool {
    ALL_CODES.contains(&code)
}
