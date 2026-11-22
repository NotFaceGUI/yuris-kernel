// Dump every function in the current program to one .c file, plus an index.
//
// Usage:
//   analyzeHeadless <proj> <name> -process <prog> -noanalysis \
//     -scriptPath tools/v555/ghidra -postScript DumpAllDecomp.java <outDir>
//
// Output:
//   <outDir>/<entry>_<name>.c
//   <outDir>/_index.tsv        entry, name, size, file, status
//   <outDir>/_summary.txt      counts and failures

import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;

import java.io.File;
import java.io.PrintWriter;

public class DumpAllDecomp extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: DumpAllDecomp <outDir>");
            return;
        }
        File outDir = new File(args[0]);
        outDir.mkdirs();

        DecompInterface ifc = new DecompInterface();
        if (!ifc.openProgram(currentProgram)) {
            println("decompiler open failed: " + ifc.getLastMessage());
            return;
        }

        PrintWriter index = new PrintWriter(new File(outDir, "_index.tsv"), "UTF-8");
        index.println("entry\tname\tsize\tfile\tstatus");
        int total = 0;
        int ok = 0;
        int failed = 0;
        int skipped = 0;

        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
        while (it.hasNext()) {
            Function f = it.next();
            total++;
            String entry = f.getEntryPoint().toString();
            String name = f.getName();
            long size = f.getBody().getNumAddresses();
            String safe = name.replaceAll("[^A-Za-z0-9_.-]", "_");
            String fname = entry + "_" + safe + ".c";
            File fh = new File(outDir, fname);

            if (f.isExternal() || f.isThunk()) {
                skipped++;
                index.println(entry + "\t" + name + "\t" + size + "\t" + fname + "\tSKIP_EXTERNAL_OR_THUNK");
                continue;
            }
            try (PrintWriter pw = new PrintWriter(fh, "UTF-8")) {
                pw.println("// name: " + name);
                pw.println("// entry: " + entry);
                pw.println("// body: " + size + " bytes");
                DecompileResults res = ifc.decompileFunction(f, 300, monitor);
                if (res != null && res.decompileCompleted() && res.getDecompiledFunction() != null) {
                    pw.println(res.getDecompiledFunction().getC());
                    ok++;
                    index.println(entry + "\t" + name + "\t" + size + "\t" + fname + "\tOK");
                } else {
                    String msg = res == null ? "null"
                            : (res.getErrorMessage() + " completed=" + res.decompileCompleted());
                    pw.println("// DECOMPILE FAILED: " + msg);
                    failed++;
                    index.println(entry + "\t" + name + "\t" + size + "\t" + fname + "\tFAILED:" + msg);
                }
            } catch (Exception ex) {
                failed++;
                println("exception " + entry + ": " + ex);
                index.println(entry + "\t" + name + "\t" + size + "\t" + fname + "\tEXCEPTION:" + ex);
            }
            if (total % 250 == 0) {
                println("processed " + total + " functions (ok " + ok + ", failed " + failed + ")");
            }
        }
        index.close();
        try (PrintWriter summary = new PrintWriter(new File(outDir, "_summary.txt"), "UTF-8")) {
            summary.println("program=" + currentProgram.getName());
            summary.println("total=" + total);
            summary.println("ok=" + ok);
            summary.println("failed=" + failed);
            summary.println("skipped_external_or_thunk=" + skipped);
        }
        ifc.dispose();
        println("DumpAllDecomp done: total=" + total + " ok=" + ok
                + " failed=" + failed + " skipped=" + skipped);
    }
}
