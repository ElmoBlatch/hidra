import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import java.io.FileWriter;
import java.io.PrintWriter;

public class DecompileAt extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/decompiled_functions.txt";
        String[] addresses = {"1409a8a70", "1409a8b40", "1409bc0a0", "14092f3cc"};
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            for (String addr : addresses) {
                Address a = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(addr);
                Function f = currentProgram.getListing().getFunctionAt(a);
                out.println("\n===== " + addr + " : " + (f == null ? "NO_FUNCTION" : f.getName()) + " =====");
                if (f == null) continue;
                DecompileResults r = decomp.decompileFunction(f, 60, monitor);
                if (r != null && r.decompileCompleted() && r.getDecompiledFunction() != null) out.println(r.getDecompiledFunction().getC());
                else out.println("<decompile unavailable: " + (r == null ? "null" : r.getErrorMessage()) + ">");
            }
        } finally { decomp.dispose(); }
        println("Wrote selected decompilations to " + outPath);
    }
}
