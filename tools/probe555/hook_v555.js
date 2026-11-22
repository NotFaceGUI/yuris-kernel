// v555 official-engine probe: tokenizer + variable writers + expression getters.
// Logs only filtered records to keep the stream manageable.

const mod = Process.enumerateModules()[0];
const base = mod.base;
const VA = (a) => base.add(a - 0x400000);
send({ type: 'base', name: mod.name, base: base.toString() });

function u32(p) { try { return ptr(p).readU32(); } catch (e) { return null; } }
function num(v) { try { return (typeof v === 'number') ? v : ptr(v).toUInt32(); } catch (e) { return -1; } }
function cstr(p, n) { try { return ptr(p).readCString(Math.min(n, 120)); } catch (e) { return null; } }


const VAR_TABLE_PTR = 0x87258c;
function varRec(id) {
  try {
    const table = ptr(VAR_TABLE_PTR).readU32();
    return ptr(ptr(table).add(id * 4).readU32());
  } catch (e) { return null; }
}
function snapVar(id, isStr) {
  const rec = varRec(id);
  if (rec === null) return null;
  try {
    const ty = rec.add(1).readU8();
    const out = { id: id, ty: ty };
    if (ty === 3) {
      const data = ptr(rec.add(0x38).readU32());
      const lens = ptr(rec.add(0x3c).readU32());
      out.items = [];
      for (let i = 0; i < 6; i++) {
        const ptrs = data.add(i * 4).readU32();
        const len = lens.add(i * 2).readU16();
        let text = null;
        try { text = ptr(ptrs).readUtf8String(Math.min(len + 1, 80)); } catch (e) {}
        out.items.push([len, text]);
      }
    } else {
      const data = ptr(rec.add(0x30).readU32());
      out.items = [];
      for (let i = 0; i < 6; i++) out.items.push(data.add(i * 8).readS64().toString());
    }
    return out;
  } catch (e) { return { id: id, err: e.message }; }
}

const INTERESTING = new Set([
  2912, 2913, 2914, 2915, 2916, 2917, 2918, 2919, 2920,
  2974, 2975, 2976, 2999, 3069, 3070, 3072, 3092, 3094, 3096,
  2872, 2873, 2874, 2875, 2876, 2877,
]);

let counts = {};
function tick(k) { counts[k] = (counts[k] || 0) + 1; return counts[k]; }

// 1) PRODUCT COM=22 tokenizer entry/exit.
let tokN = 0;
Interceptor.attach(VA(0x45a9dc), {
  onEnter() {
    this.n = tokN++;
    this.esp = ptr(this.context.esp);
    this.a = [];
    for (let i = 0; i < 8; i++) this.a.push(u32(this.esp.add(4 + i * 4)));
    this.line = cstr(this.context.eax, 200);
    if (this.n === 0) {
      const snap = {};
      for (const id of [2912,2913,2914,2915,2916,2917,2918,2919,2920,2921,2922,2923,2924,2925,2969,2970,2971,2972,2973,2974,2975,2976,2977,2999,3000,3001,3002]) {
        snap[id] = snapVar(id, true);
      }
      send({ type: 'snap0', vars: snap });
    }
  },
  onLeave(retval) {
    if (this.n < 60 || this.n % 500 === 0) {
      send({ type: 'tok', n: this.n, args: this.a, line: this.line, ret: retval.toString() });
    }
    if (this.n < 3) {
      const after = {};
      for (const id of [2912,2913,2914,2915,2916,2917,2918,2919,2920,2921,2922,2923,2924,2925,2974,2975,2976,2999]) after[id] = snapVar(id);
      send({ type: 'snap_after', n: this.n, vars: after });
    }
  }
});

// 2) String writer FUN_0045bb60: EAX=slot, EDX=index, ECX=string.
let swN = 0;
Interceptor.attach(VA(0x45bb60), {
  onEnter() {
    const slot = num(this.context.eax);
    const idx = num(this.context.edx);
    if ((INTERESTING.has(slot) && idx < 12 && swN < 8000) || swN < 10) {
      send({ type: 'sw', n: swN, slot: slot, idx: idx, s: cstr(this.context.ecx, 80) });
    }
    swN++;
  }
});

// 3) Number writer FUN_0045bc60: EAX=slot, EDX=index, value on stack.
let nwN = 0;
Interceptor.attach(VA(0x45bc60), {
  onEnter() {
    const slot = num(this.context.eax);
    const idx = num(this.context.edx);
    if ((INTERESTING.has(slot) && idx < 12 && nwN < 8000) || nwN < 10) {
      const esp = ptr(this.context.esp);
      send({
        type: 'nw', n: nwN, slot: slot, idx: idx,
        w4: u32(esp.add(4)), w8: u32(esp.add(8)),
        wc: u32(esp.add(0xc)), w10: u32(esp.add(0x10)),
      });
    }
    nwN++;
  }
});

// 4) Expression integer getter FUN_004595f8: EAX=slot, [esp+4]=index.
let igN = 0;
Interceptor.attach(VA(0x4595f8), {
  onEnter() {
    const slot = num(this.context.eax);
    const esp = ptr(this.context.esp);
    const idx = u32(esp.add(8));
    if (slot >= 1000 && igN < 4000) {
      send({ type: 'ig', n: igN, slot: slot, idx: idx });
    }
    igN++;
  }
});

// 5) Sysvar int getter FUN_0044803c: EAX=id; [esp+0xa8]=slot-ish arg.
let sgN = 0;
Interceptor.attach(VA(0x44803c), {
  onEnter() {
    const id = num(this.context.eax);
    const esp = ptr(this.context.esp);
    const a = u32(esp.add(0xa8));
    const b = u32(esp.add(0xa4));
    if ((id === 50 || id === 52) && sgN < 2000) {
      send({ type: 'sg', n: sgN, id: id, a: a, b: b });
    }
    sgN++;
  }
});

// 6) TEXT handler: log FILE/LET slot ids and return.
let txN = 0;
Interceptor.attach(VA(0x452a0c), {
  onEnter() { this.esp = ptr(this.context.esp); },
  onLeave(retval) {
    if (txN < 80) {
      send({ type: 'text', n: txN, ret: retval.toString() });
    }
    txN++;
  }
});

send({ type: 'ready' });
