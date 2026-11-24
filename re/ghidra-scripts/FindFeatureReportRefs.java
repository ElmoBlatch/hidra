import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import java.io.FileWriter;
import java.io.PrintWriter;

public class FindFeatureReportRefs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/feature_report_refs.txt";
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            DataIterator it = currentProgram.getListing().getDefinedData(true);
            while (it.hasNext()) {
                Data d = it.next();
                Object value = d.getValue();
                if (!(value instanceof String)) continue;
                String s = (String)value;
                if (!s.contains("FeatureReport") && !s.contains("HID++ 2.0 notification") && !s.contains("unexpected HID++")) continue;
                out.println("\nSTRING " + d.getAddress() + " " + s);
                ReferenceIterator refs = currentProgram.getReferenceManager().getReferencesTo(d.getAddress());
                int count = 0;
                while (refs.hasNext()) {
                    Reference ref = refs.next(); count++;
                    Address from = ref.getFromAddress();
                    Function f = currentProgram.getListing().getFunctionContaining(from);
                    out.println("  ref=" + ref.getReferenceType() + " from=" + from + " function=" + (f == null ? "-" : f.getName() + "@" + f.getEntryPoint()));
                }
                out.println("  count=" + count);
            }
        }
        println("Wrote FeatureReport string refs to " + outPath);
    }
}
