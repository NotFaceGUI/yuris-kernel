// Extract the v555 runtime command-handler table from a PE build.
//
// The table lives in BSS and is filled at startup by an initializer such as:
//   for (i=0; i<121; i++) table[i] = default_handler;
//   table[0] = 0x423168; table[1] = ...;
//
// Ghidra does not create functions for handlers referenced only by that data
// table, so this script first recovers the table, then creates functions for
// every distinct handler address.
//
// Usage:
//   analyzeHeadless <proj> <name> -process <prog> -noanalysis \
//     -scriptPath tools/v555/ghidra -postScript ExtractDispatch.java \
//     <outDir> [tableBaseHex] [initFuncHex]
//
// Output:
//   <outDir>/dispatch_raw.tsv   opcode, handler, explicit
//   <outDir>/handlers.txt       distinct handler addresses
//   <outDir>/dispatch.log       candidate / default / count diagnostics

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.scalar.Scalar;

import java.io.File;
import java.io.PrintWriter;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;
import java.util.TreeSet;

public class ExtractDispatch extends GhidraScript {

    private long dstAbs(Object[] ops) {
        if (ops == null || ops.length == 0) {
            return -1;
        }
        if (ops.length == 1 && ops[0] instanceof Address) {
            return ((Address) ops[0]).getOffset();
        }
        // [reg + imm]
        if (ops.length == 2 && ops[0] instanceof ghidra.program.model.lang.Register
                && ops[1] instanceof Scalar) {
            return ((Scalar) ops[1]).getUnsignedValue();
        }
        return -1;
    }

    private boolean isWritable(long addr) {
        Address a = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(addr);
        MemoryBlock b = currentProgram.getMemory().getBlock(a);
        if (b == null) {
            return false;
        }
        return b.isWrite() && !b.isExecute();
    }

    private boolean isExecutable(long addr) {
        Address a = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(addr);
        MemoryBlock b = currentProgram.getMemory().getBlock(a);
        return b != null && b.isExecute();
    }

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: ExtractDispatch <outDir> [tableBaseHex] [initFuncHex]");
            return;
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();
        long wantBase = args.length > 1 ? Long.parseLong(args[1].replace("0x", ""), 16) : -1;
        long wantInit = args.length > 2 ? Long.parseLong(args[2].replace("0x", ""), 16) : -1;

        PrintWriter log = new PrintWriter(new File(outDir, "dispatch.log"), "UTF-8");
        FunctionManager fm = currentProgram.getFunctionManager();
        Listing listing = currentProgram.getListing();

        // function entry -> (dst -> imm), plus structural hints.
        Map<Long, TreeMap<Long, Long>> perFunc = new LinkedHashMap<>();
        Map<Long, Function> funcs = new HashMap<>();
        Map<Long, TreeSet<Long>> loopBases = new HashMap<>();
        Map<Long, Integer> cmpCounts = new HashMap<>();
        for (Function f : fm.getFunctions(true)) {
            long entry = f.getEntryPoint().getOffset();
            if (wantInit >= 0 && entry != wantInit) {
                continue;
            }
            TreeMap<Long, Long> hits = new TreeMap<>();
            TreeSet<Long> loops = new TreeSet<>();
            int cmpCount = 0;
            InstructionIterator it = listing.getInstructions(f.getBody(), true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                String mn = ins.getMnemonicString().toUpperCase();
                if (mn.equals("CMP")) {
                    Object[] ops = ins.getOpObjects(1);
                    if (ops != null && ops.length == 1 && ops[0] instanceof Scalar) {
                        long v = ((Scalar) ops[0]).getUnsignedValue();
                        if (v == 0x1E4 || v == 0x1E8 || v == 0x1E0) {
                            cmpCount = (int) (v / 4);
                        }
                    }
                }
                if (!mn.equals("MOV")) {
                    continue;
                }
                Object[] dst = ins.getOpObjects(0);
                Object[] src = ins.getOpObjects(1);
                if (src == null || src.length != 1 || !(src[0] instanceof Scalar)) {
                    continue;
                }
                long imm = ((Scalar) src[0]).getUnsignedValue();
                long d = dstAbs(dst);
                if (d < 0 || imm == 0) {
                    continue;
                }
                if (isWritable(d) && isExecutable(imm)) {
                    hits.put(d, imm);
                }
                // Loop store: MOV [reg + base], default ; base is a BSS address
                // and the immediate is an executable body.
                if (dst != null && dst.length == 2 && dst[1] instanceof Scalar
                        && isExecutable(imm)) {
                    long base = ((Scalar) dst[1]).getUnsignedValue();
                    if (isWritable(base)) {
                        loops.add(base);
                    }
                }
            }
            if (!hits.isEmpty()) {
                perFunc.put(entry, hits);
                funcs.put(entry, f);
                loopBases.put(entry, loops);
                cmpCounts.put(entry, cmpCount);
            }
        }

        // Choose the initializer. The real table is filled by a loop over
        // exactly 121 slots, so a loop default and a CMP bound are strong
        // signals; raw explicit-write count alone can pick an unrelated array.
        long bestInit = -1;
        long bestBase = -1;
        int bestCount = -1;
        int bestScore = Integer.MIN_VALUE;
        for (Map.Entry<Long, TreeMap<Long, Long>> e : perFunc.entrySet()) {
            TreeMap<Long, Long> hits = e.getValue();
            int cmpCount = cmpCounts.getOrDefault(e.getKey(), 0);
            TreeSet<Long> loops = loopBases.getOrDefault(e.getKey(), new TreeSet<>());
            for (long cand : hits.keySet()) {
                for (int k = 0; k <= 8; k++) {
                    long base = cand - (long) k * 4;
                    if (wantBase >= 0 && base != wantBase) {
                        continue;
                    }
                    int count = 0;
                    TreeSet<Long> slots = new TreeSet<>();
                    for (long d : hits.keySet()) {
                        if (d >= base && d < base + 0x400 && ((d - base) & 3) == 0) {
                            count++;
                            slots.add((d - base) / 4);
                        }
                    }
                    int score = count;
                    if (loops.contains(base)) {
                        score += 1000;
                    }
                    if (cmpCount == 121) {
                        score += 500;
                    }
                    if (!slots.isEmpty() && slots.last() < 121 && slots.size() >= 40) {
                        score += 100;
                    }
                    if (score > bestScore) {
                        bestScore = score;
                        bestCount = count;
                        bestBase = base;
                        bestInit = e.getKey();
                    }
                }
            }
        }
        if (wantBase >= 0) {
            // Re-scan against the requested base in case no dense cluster matched.
            bestBase = wantBase;
            bestInit = wantInit;
            bestCount = 0;
            for (Map.Entry<Long, TreeMap<Long, Long>> e : perFunc.entrySet()) {
                for (long d : e.getValue().keySet()) {
                    if (d >= bestBase && d < bestBase + 0x400) {
                        bestCount++;
                        if (bestInit < 0) {
                            bestInit = e.getKey();
                        }
                    }
                }
            }
        }
        log.println("initializer=0x" + Long.toHexString(bestInit) + " tableBase=0x"
                + Long.toHexString(bestBase) + " explicitInRange=" + bestCount);

        // Find loop bound (CMP reg, 0x1e4 -> 121) and default handler.
        int count = 121;
        long defaultHandler = 0;
        if (bestInit >= 0 && funcs.containsKey(bestInit)) {
            Function f = funcs.get(bestInit);
            InstructionIterator it = listing.getInstructions(f.getBody(), true);
            while (it.hasNext()) {
                Instruction ins = it.next();
                String mn = ins.getMnemonicString().toUpperCase();
                if (mn.equals("CMP")) {
                    Object[] ops = ins.getOpObjects(1);
                    if (ops != null && ops.length == 1 && ops[0] instanceof Scalar) {
                        long v = ((Scalar) ops[0]).getUnsignedValue();
                        if (v == 0x1E4 || v == 0x1E8 || v == 0x1E0) {
                            count = (int) (v / 4);
                            log.println("count from CMP: " + count + " (0x" + Long.toHexString(v) + ")");
                        }
                    }
                }
                if (mn.equals("MOV") && defaultHandler == 0) {
                    Object[] dst = ins.getOpObjects(0);
                    Object[] src = ins.getOpObjects(1);
                    if (src != null && src.length == 1 && src[0] instanceof Scalar) {
                        long imm = ((Scalar) src[0]).getUnsignedValue();
                        long d = dstAbs(dst);
                        if (d == bestBase && isExecutable(imm)) {
                            defaultHandler = imm;
                            log.println("default handler from loop: 0x" + Long.toHexString(imm));
                        }
                    }
                }
            }
        }
        if (defaultHandler == 0) {
            // Fallback: most frequent handler value among all explicit entries.
            Map<Long, Integer> freq = new HashMap<>();
            for (TreeMap<Long, Long> hits : perFunc.values()) {
                for (Map.Entry<Long, Long> h : hits.entrySet()) {
                    long d = h.getKey();
                    if (d >= bestBase && d < bestBase + (long) count * 4) {
                        freq.merge(h.getValue(), 1, Integer::sum);
                    }
                }
            }
            for (Map.Entry<Long, Integer> e : freq.entrySet()) {
                if (defaultHandler == 0 || e.getValue() > freq.get(defaultHandler)) {
                    defaultHandler = e.getKey();
                }
            }
            log.println("default handler from frequency: 0x" + Long.toHexString(defaultHandler));
        }

        long[] handlers = new long[count];
        boolean[] explicit = new boolean[count];
        for (int i = 0; i < count; i++) {
            handlers[i] = defaultHandler;
        }
        for (TreeMap<Long, Long> hits : perFunc.values()) {
            for (Map.Entry<Long, Long> h : hits.entrySet()) {
                long d = h.getKey();
                if (d >= bestBase && d < bestBase + (long) count * 4 && ((d - bestBase) & 3) == 0) {
                    int idx = (int) ((d - bestBase) / 4);
                    handlers[idx] = h.getValue();
                    explicit[idx] = true;
                }
            }
        }

        // Create functions for every distinct handler (data-table-only entries
        // are not auto-created by Ghidra).
        TreeSet<Long> distinct = new TreeSet<>();
        for (long h : handlers) {
            if (isExecutable(h)) {
                distinct.add(h);
            }
        }
        int created = 0;
        for (long h : distinct) {
            Address a = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(h);
            if (fm.getFunctionAt(a) != null) {
                continue;
            }
            try {
                disassemble(a);
            } catch (Exception ex) {
                log.println("disassemble 0x" + Long.toHexString(h) + ": " + ex);
            }
            try {
                Function nf = createFunction(a, "V555H_" + Long.toHexString(h));
                if (nf != null) {
                    created++;
                }
            } catch (Exception ex) {
                log.println("createFunction 0x" + Long.toHexString(h) + ": " + ex);
            }
        }
        log.println("distinct handlers=" + distinct.size() + " created=" + created);

        PrintWriter tsv = new PrintWriter(new File(outDir, "dispatch_raw.tsv"), "UTF-8");
        tsv.println("opcode\thandler\texplicit");
        for (int i = 0; i < count; i++) {
            tsv.println(i + "\t0x" + String.format("%08x", handlers[i]) + "\t" + (explicit[i] ? 1 : 0));
        }
        tsv.close();

        PrintWriter list = new PrintWriter(new File(outDir, "handlers.txt"), "UTF-8");
        for (long h : distinct) {
            list.println(String.format("%08x", h));
        }
        list.close();
        log.println("wrote dispatch_raw.tsv / handlers.txt");
        log.close();
        println("ExtractDispatch done: tableBase=0x" + Long.toHexString(bestBase)
                + " count=" + count + " default=0x" + Long.toHexString(defaultHandler)
                + " distinct=" + distinct.size());
    }
}
