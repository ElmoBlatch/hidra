import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.LinkedHashMap;
import java.util.Map;

public class FindTransportCallers extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/transport_callers.txt";
        String[][] targets = {{"HidSetFeature", "1406d9150"}, {"HidGetFeature", "1406d9240"}, {"HidGetFeatureInPlace", "1406d9320"}, {"WinUsbWritePipe", "14043a400"}};
        Map<Function, StringBuilder> callers = new LinkedHashMap<>();
        for (String[] target : targets) {
            Address address = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(target[1]);
            ReferenceIterator refs = currentProgram.getReferenceManager().getReferencesTo(address);
            while (refs.hasNext()) {
                Reference ref = refs.next();
                Function f = currentProgram.getListing().getFunctionContaining(ref.getFromAddress());
                if (f == null || f.getEntryPoint().equals(address)) continue;
                StringBuilder b = callers.get(f);
                if (b == null) { b = new StringBuilder(); callers.put(f, b); }
                b.append("  ").append(target[0]).append(" @ ").append(address).append(" <- ").append(ref.getFromAddress()).append("\n");
            }
        }
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            out.println("Target: " + currentProgram.getName());
            out.println("Immediate transport callers: " + callers.size());
            for (Map.Entry<Function, StringBuilder> e : callers.entrySet()) {
                out.println("\n===== " + e.getKey().getName() + " @ " + e.getKey().getEntryPoint() + " =====");
                out.print(e.getValue());
                DecompileResults result = decomp.decompileFunction(e.getKey(), 60, monitor);
                if (result != null && result.decompileCompleted() && result.getDecompiledFunction() != null) out.println(result.getDecompiledFunction().getC());
                else out.println("<decompile unavailable: " + (result == null ? "null" : result.getErrorMessage()) + ">");
            }
        } finally { decomp.dispose(); }
        println("Wrote " + callers.size() + " transport caller dossiers to " + outPath);
    }
}
