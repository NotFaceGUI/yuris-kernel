// 列出一个或多个地址的引用者(取证:扩展名/字符串常量被哪些函数引用)。
//
// 用法:
//   analyzeHeadless <proj> <name> -process <prog> -noanalysis \
//     -scriptPath tools/v555/ghidra -postScript ListXrefs.java 00401000 [00401004 ...]
//
// 输出每个地址的 from 地址、引用类型与所在函数(未知函数则留空)。

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;

public class ListXrefs extends GhidraScript {

    @Override
    public void run() throws Exception {
        String[] args = getScriptArgs();
        if (args.length < 1) {
            println("usage: ListXrefs <addr> [addr...]");
            return;
        }
        for (String s : args) {
            Address to = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(s);
            println("== xrefs to " + to);
            ReferenceIterator it = currentProgram.getReferenceManager().getReferencesTo(to);
            int n = 0;
            while (it.hasNext()) {
                Reference ref = it.next();
                Address from = ref.getFromAddress();
                Function f = currentProgram.getFunctionManager().getFunctionContaining(from);
                println("  from=" + from + " type=" + ref.getReferenceType()
                        + " func=" + (f == null ? "?" : f.getName() + "@" + f.getEntryPoint()));
                n++;
            }
            println("  total=" + n);
        }
    }
}
