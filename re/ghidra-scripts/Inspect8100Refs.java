import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;
import java.io.FileWriter;
import java.io.PrintWriter;

public class Inspect8100Refs extends GhidraScript {
    @Override
    public void run() throws Exception {
        String outPath = getScriptArgs().length > 0 ? getScriptArgs()[0] : "/tmp/logitech-re/8100_refs.txt";
        String[][] targets = {
            {"GetOnboardProfilesInfo", "14092d310"}, {"ReadData", "14092d9f0"},
            {"BeginWrite", "14092dbc0"}, {"WriteData", "14092dd20"}, {"EndWrite", "14092df00"},
            {"SetActiveProfile", "14092d790"}, {"GetCRC", "14092e100"}
        };
        try (PrintWriter out = new PrintWriter(new FileWriter(outPath))) {
            for (String[] target : targets) {
                Address address = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(target[1]);
                out.println("\n== " + target[0] + " @ " + address + " ==");
                ReferenceIterator refs = currentProgram.getReferenceManager().getReferencesTo(address);
                int count = 0;
                while (refs.hasNext()) {
                    Reference ref = refs.next(); count++;
                    Address from = ref.getFromAddress();
                    Data data = currentProgram.getListing().getDataContaining(from);
                    Instruction ins = currentProgram.getListing().getInstructionContaining(from);
                    Function func = currentProgram.getListing().getFunctionContaining(from);
                    out.println("ref=" + ref.getReferenceType() + " from=" + from +
                        " data=" + (data == null ? "-" : data.getDataType().getName()) +
                        " ins=" + (ins == null ? "-" : ins.getMnemonicString()) +
                        " function=" + (func == null ? "-" : func.getName() + "@" + func.getEntryPoint()));
                }
                out.println("count=" + count);
            }
        }
        println("Wrote 0x8100 method references to " + outPath);
    }
}
