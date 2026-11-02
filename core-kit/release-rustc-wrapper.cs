using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Text;

// Preserve Cargo's Windows arguments while adding the per-crate generated root.
internal static class ReleaseRustcWrapper {
    private static string Quote(string value) {
        var result = new StringBuilder("\"");
        int slashes = 0;
        foreach (char ch in value) {
            if (ch == '\\') { slashes++; continue; }
            result.Append('\\', ch == '"' ? 2 * slashes + 1 : slashes);
            result.Append(ch);
            slashes = 0;
        }
        result.Append('\\', 2 * slashes);
        return result.Append('"').ToString();
    }
    public static int Main(string[] args) {
        if (args.Length == 0) return 2;
        var flags = new List<string>();
        for (int i = 1; i < args.Length; i++) flags.Add(args[i]);
        string output = Environment.GetEnvironmentVariable("OUT_DIR");
        if (!String.IsNullOrEmpty(output)) {
            flags.Add("--remap-path-prefix=" + output + "=/topaz/out");
            flags.Add("--remap-path-prefix=" + output.Replace('\\', '/') + "=/topaz/out");
        }
        var command = new StringBuilder();
        foreach (string flag in flags) {
            if (command.Length != 0) command.Append(' ');
            command.Append(Quote(flag));
        }
        try {
            using (var child = Process.Start(new ProcessStartInfo(args[0], command.ToString()) {UseShellExecute = false})) {
                child.WaitForExit();
                return child.ExitCode;
            }
        } catch {
            Console.Error.WriteLine("release rustc wrapper could not start the selected compiler");
            return 1;
        }
    }
}
