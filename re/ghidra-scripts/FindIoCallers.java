import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileResults;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.ExternalLocation;
import ghidra.program.model.symbol.ExternalManager;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import java.io.FileWriter;
import java.io.PrintWriter;
import java.util.LinkedHashMap;
import java.util.Map;

public class FindIoCallers extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/io_callers.txt";
        String[][] wanted = {
            {"HID.DLL", "HidD_SetFeature"}, {"HID.DLL", "HidD_GetFeature"},
            {"HID.DLL", "HidD_SetOutputReport"}, {"HID.DLL", "HidD_GetInputReport"},
            {"WINUSB.DLL", "WinUsb_WritePipe"}, {"WINUSB.DLL", "WinUsb_ReadPipe"}
        };
        Map<Function, StringBuilder> callers = new LinkedHashMap<>();
        ExternalManager em = currentProgram.getExternalManager();
        for (String[] item : wanted) {
            for (ExternalLocation loc : em.getExternalLocations(item[0], item[1])) {
                ReferenceIterator refs = currentProgram.getReferenceManager().getReferencesTo(loc.getAddress());
                while (refs.hasNext()) {
                    Reference ref = refs.next();
                    Function f = currentProgram.getListing().getFunctionContaining(ref.getFromAddress());
                    if (f == null) continue;
                    StringBuilder b = callers.get(f);
                    if (b == null) { b = new StringBuilder(); callers.put(f, b); }
                    b.append("  ").append(item[0]).append("!").append(item[1]).append(" <- ").append(ref.getFromAddress()).append("\n");
                }
            }
        }
        DecompInterface decomp = new DecompInterface();
        decomp.openProgram(currentProgram);
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            out.println("Target: " + currentProgram.getName());
            out.println("Native HID/WinUSB import callers: " + callers.size());
            for (Map.Entry<Function, StringBuilder> e : callers.entrySet()) {
                out.println("\n===== " + e.getKey().getName() + " @ " + e.getKey().getEntryPoint() + " =====");
                out.print(e.getValue());
                DecompileResults result = decomp.decompileFunction(e.getKey(), 60, monitor);
                if (result != null && result.decompileCompleted() && result.getDecompiledFunction() != null) out.println(result.getDecompiledFunction().getC());
                else out.println("<decompile unavailable: " + (result == null ? "null" : result.getErrorMessage()) + ">");
            }
        } finally { decomp.dispose(); }
        println("Wrote " + callers.size() + " HID/WinUSB caller dossiers to " + outPath);
    }
}
