// Ghidra headless script: find LGS profile-related strings and decompile xref owners.
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Listing;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import ghidra.program.model.symbol.ReferenceManager;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.LinkedHashMap;
import java.util.Locale;
import java.util.Map;

public class FindProfileRefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/profile_refs.txt";
        Map<Function, StringBuilder> hits = new LinkedHashMap<>();
        Listing listing = currentProgram.getListing();
        ReferenceManager refs = currentProgram.getReferenceManager();
        DataIterator it = listing.getDefinedData(true);
        int stringCount = 0;
        while (it.hasNext()) {
            Data d = it.next();
            Object value = d.getValue();
            if (!(value instanceof String)) continue;
            String s = (String) value;
            String lower = s.toLowerCase(Locale.ROOT);
            if (!(lower.contains("profile") || lower.contains("dpi") || lower.contains("resolution") || lower.contains("8100"))) continue;
            stringCount++;
            ReferenceIterator rit = refs.getReferencesTo(d.getAddress());
            while (rit.hasNext()) {
                Reference ref = rit.next();
                Function f = listing.getFunctionContaining(ref.getFromAddress());
                if (f == null) f = listing.getFunctionAt(ref.getFromAddress());
                if (f == null) continue;
                StringBuilder b = hits.get(f);
                if (b == null) { b = new StringBuilder(); hits.put(f, b); }
                b.append("  ").append(d.getAddress()).append(" <- ").append(ref.getFromAddress()).append(" : ").append(s.replace('\n', ' ')).append("\n");
            }
        }
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            out.println("Target: " + currentProgram.getName());
            out.println("Profile/DPI-related defined strings: " + stringCount);
            out.println("Functions referencing them: " + hits.size());
            for (Map.Entry<Function, StringBuilder> e : hits.entrySet()) {
                Function f = e.getKey();
                out.println("\n===== " + f.getName() + " @ " + f.getEntryPoint() + " =====");
                out.print(e.getValue());
                DecompileResults result = decomp.decompileFunction(f, 90, monitor);
                if (result != null && result.decompileCompleted() && result.getDecompiledFunction() != null) {
                    out.println(result.getDecompiledFunction().getC());
                } else {
                    out.println("<decompile unavailable: " + (result == null ? "null" : result.getErrorMessage()) + ">");
                }
            }
        } finally {
            decomp.dispose();
        }
        println("Wrote " + hits.size() + " function dossiers from " + stringCount + " matching strings to " + outPath);
    }
}
