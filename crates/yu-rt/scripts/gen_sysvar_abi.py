#!/usr/bin/env python3
"""从官方 YSCom.ycd 生成**系统变量 ABI 数据表**(`data/sysvar-abi.tsv`)。

为什么要生成这张表:
  商业发行版**不带编译器**(实测 <game-b> `data.ypf` 的 12,212 条目里
  `ycd`/`yscom` 0 命中),因此运行核不能把 `YSCom.ycd` 当作必需输入。
  但系统变量 id 空间是**编译器与引擎之间的 ABI**(编译产物里存的就是 id),
  且实测 0.494 商业包的 `ysv.ybn` 前 548 条与 0.495 SDK 的 ycd **逐项一致**
  (类型 + 维数 + 顺序,0 差异)。所以:

  * 有 ycd 的工程(SDK/开发工程)→ 直接用 ycd;
  * 只有 `.ybn` 的商业包 → 用本表,并在装载时用**工程自己的 `ysv.ybn`**
    逐项校验 (kind=1、type、dims)。任一项不符 ⇒ `table.version` 拒绝启动。
    这条校验把"表是引擎 ABI"从说法变成可复核的事实。

用法:
  python crates/yu-rt/scripts/gen_sysvar_abi.py <YSCom.ycd> \
      > crates/yu-rt/data/sysvar-abi-0495.tsv
"""
import hashlib
import os
import struct
import sys


def cstr(b, o):
    e = b.index(0, o)
    return b[o:e].decode("ascii"), e + 1


def find_sysvar_section(b):
    """与 Rust `YcdTable::parse` 同判据:段头 + 首名 `_` + 段尾紧跟段头。"""
    for off in range(0x10, len(b) - 8):
        cnt = struct.unpack_from("<I", b, off)[0]
        zero = struct.unpack_from("<I", b, off + 4)[0]
        if zero != 0 or not (1 <= cnt <= 4096):
            continue
        q = off + 8
        out = []
        ok = True
        for _ in range(cnt):
            try:
                name, q = cstr(b, q)
            except ValueError:
                ok = False
                break
            if not name.startswith("_"):
                ok = False
                break
            if q + 2 > len(b):
                ok = False
                break
            ty, dims = b[q], b[q + 1]
            q += 2
            bounds = []
            for _ in range(dims):
                bounds.append(struct.unpack_from("<I", b, q)[0])
                q += 4
            if q > len(b):
                ok = False
                break
            out.append((name, ty, bounds))
        if not ok:
            continue
        if q + 8 <= len(b):
            nxt = struct.unpack_from("<I", b, q)[0]
            nz = struct.unpack_from("<I", b, q + 4)[0]
            if nz == 0 and 1 <= nxt <= 1_000_000:
                return out
    raise SystemExit("找不到系统变量段")


def main():
    if len(sys.argv) > 1:
        path = sys.argv[1]
    else:
        sdk = os.environ.get("YURIS_SAMPLE_SDK0495")
        if not sdk:
            raise SystemExit(
                "用法: gen_sysvar_abi.py <YSCom.ycd> 或设置 YURIS_SAMPLE_SDK0495"
            )
        path = os.path.join(sdk, "システム", "system", "YSCom", "YSCom.ycd")
    b = open(path, "rb").read()
    version = struct.unpack_from("<I", b, 4)[0]
    sha = hashlib.sha256(b).hexdigest()
    decls = find_sysvar_section(b)
    out = []
    out.append("# sysvar ABI table (engine ABI, not game data)")
    out.append(f"# source_ycd_version\t{version}")
    out.append(f"# source_ycd_sha256\t{sha}")
    out.append(f"# count\t{len(decls)}")
    out.append("# columns: id<TAB>name<TAB>value_type(1=int,2=flt,3=str)<TAB>dims(comma)")
    for i, (name, ty, dims) in enumerate(decls):
        out.append(f"{i}\t{name}\t{ty}\t{','.join(str(d) for d in dims)}")
    sys.stdout.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
