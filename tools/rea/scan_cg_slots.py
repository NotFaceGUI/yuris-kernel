#!/usr/bin/env python3
"""官方 0.495 CG/CGACT 参数槽位读取扫描（只读,静态）。

用途:定位 0.495 的 CG/CGACT 命令处理体(入口 `0x42a514` / `0x42c4a4`,
动作处理体 `0x42f809..0x43164a`)里每一处对参数值数组
`[slot*8 + 0x683740]` 的读取,并把「槽号 → 读取地址」列成表,
作为 DIV-GFX-003/011/031/039 的槽位绑定证据。

两种读取形态都要认(漏第二种会得出「参数从不被读取」的错误结论):

1. 绝对寻址:`movzx eax, byte ptr [0x684161]` → 声明下标 1;
2. 立即数地址 + 间接解引用:`mov edx, 0x684181` → `movzx ecx, [edx]`
   (例如 CG 的 `FILE`、CGACT 的 `SIZE` 就是这种形态)。

参数值数组布局(0.495):
  `[0x684160 + 声明下标]` = 槽号(每命令执行前 memset 100 字节再填)
  `[slot*8 + 0x683740]`   = 值低 32 位
  `[slot*8 + 0x683744]`   = 值高 32 位(浮点高半 / 类型标记,零判定会 OR 进来)

用法:
  python tools/rea/scan_cg_slots.py <yu-ris.exe> [起址] [止址]
默认区间 0x42a000..0x431700。输出为文本表 + 「未读声明下标」汇总;
汇总里超出该命令参数条数的下标是另一命令的噪音(CG=45 个、CGACT=70 个),
按命令裁剪后再判断。关键地址再用 REA `inspect_native_instruction` 复核。
"""

import struct
import sys

import capstone

PARAM_BASE = 0x683740
SLOT_TABLE = 0x684160
SLOT_TABLE_LEN = 0x68


def load_pe(path):
    data = open(path, "rb").read()
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[e_lfanew : e_lfanew + 4] == b"PE\0\0", "not PE"
    coff = e_lfanew + 4
    nsec, = struct.unpack_from("<H", data, coff + 2)
    opt_size, = struct.unpack_from("<H", data, coff + 16)
    opt = coff + 20
    magic, = struct.unpack_from("<H", data, opt)
    assert magic == 0x10B, "not PE32"
    image_base, = struct.unpack_from("<I", data, opt + 28)
    sec = opt + opt_size
    sections = []
    for i in range(nsec):
        off = sec + i * 40
        name = data[off : off + 8].rstrip(b"\0").decode("latin1")
        vsize, va, rawsize, rawptr = struct.unpack_from("<IIII", data, off + 8)
        sections.append((name, va, vsize, rawptr, rawsize))
    return data, image_base, sections


def va_to_off(sections, image_base, va):
    rva = va - image_base
    for name, sva, vsize, rawptr, rawsize in sections:
        if sva <= rva < sva + max(vsize, rawsize):
            delta = rva - sva
            if delta >= rawsize:
                return None
            return rawptr + delta
    return None


def main():
    if len(sys.argv) < 2:
        print("usage: scan_cg_slots.py <yu-ris.exe> [start] [end]", file=sys.stderr)
        return 2
    path = sys.argv[1]
    start = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0x42A000
    end = int(sys.argv[3], 0) if len(sys.argv) > 3 else 0x431700
    data, image_base, sections = load_pe(path)
    off = va_to_off(sections, image_base, start)
    if off is None:
        print("start not mapped", file=sys.stderr)
        return 2
    code = data[off : off + (end - start)]

    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    insns = list(md.disasm(code, start))

    def written_reg(ins):
        """保守判定:返回本指令显式写入的寄存器 id(只看首操作数)。"""
        if not ins.operands:
            return None
        dst = ins.operands[0]
        if dst.type != capstone.x86.X86_OP_REG:
            return None
        if dst.access and not (dst.access & capstone.CS_AC_WRITE):
            return None
        return dst.reg

    def slot_index(disp):
        """0x6841xx → 声明下标;不在表内返回 None。"""
        delta = disp - SLOT_TABLE
        if 0 <= delta < SLOT_TABLE_LEN:
            return delta
        return None

    rows = []
    indirect = []
    for i, ins in enumerate(insns):
        # 形态 1:绝对寻址的槽表读取
        for op in ins.operands:
            if op.type != capstone.x86.X86_OP_MEM:
                continue
            if op.mem.base != 0 or op.mem.index != 0:
                continue
            idx = slot_index(op.mem.disp)
            if idx is None:
                continue
            rows.append((ins.address, idx, "abs", ins.mnemonic, ins.op_str))
        # 形态 2:`mov reg, 0x6841xx` —— 后续 `[reg]` 即同一槽表读取
        if ins.mnemonic == "mov" and len(ins.operands) == 2:
            dst, src = ins.operands
            if (
                dst.type == capstone.x86.X86_OP_REG
                and src.type == capstone.x86.X86_OP_IMM
            ):
                idx = slot_index(src.imm & 0xFFFFFFFF)
                if idx is not None:
                    indirect.append((ins.address, idx, "imm", ins.mnemonic, ins.op_str))
                    rows.append((ins.address, idx, "imm", ins.mnemonic, ins.op_str))

    # 形态 3:参数值数组的读取(槽号已知时给出下标来源)
    value_rows = []
    for i, ins in enumerate(insns):
        for op in ins.operands:
            if op.type != capstone.x86.X86_OP_MEM:
                continue
            disp = op.mem.disp
            if not (PARAM_BASE <= disp < PARAM_BASE + 0x400):
                continue
            src = "?"
            idx = op.mem.index
            if op.mem.base == 0 and idx == 0:
                src = "abs"
            elif idx != 0 and op.mem.scale == 8:
                for j in range(i - 1, max(-1, i - 24), -1):
                    prev = insns[j]
                    if written_reg(prev) == idx:
                        if (
                            prev.mnemonic == "mov"
                            and len(prev.operands) == 2
                            and prev.operands[1].type == capstone.x86.X86_OP_IMM
                        ):
                            slot = prev.operands[1].imm & 0xFFFFFFFF
                            src = f"slot_imm={slot}@0x{prev.address:x}"
                        else:
                            src = f"calc@0x{prev.address:x}"
                        break
            value_rows.append((ins.address, disp % 8, ins.mnemonic, ins.op_str, src))

    print(f"{'addr':>10} {'idx':>4} {'form':>4}  insn")
    for addr, idx, form, mn, ops in rows:
        print(f"0x{addr:08x} {idx:>4} {form:>4}  {mn} {ops}")

    seen = {idx for _, idx, _, _, _ in rows}
    missing = [i for i in range(SLOT_TABLE_LEN) if i not in seen]
    print(f"-- 声明下标已读 {sorted(seen)}")
    print(f"-- 声明下标未读 {missing}")

    print(f"\n{'addr':>10} {'half':>4}  insn   | slot source")
    for addr, half, mn, ops, src in value_rows:
        print(f"0x{addr:08x} {half:>4}  {mn} {ops}   | {src}")
    print(f"-- {len(value_rows)} value reads in 0x{start:x}..0x{end:x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
