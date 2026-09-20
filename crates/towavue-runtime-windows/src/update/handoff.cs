// Compiled by the system Windows PowerShell host after the application has
// authenticated the payload. This helper has no application/FFmpeg dependency.
// Its public key is substituted from the same tracked blob embedded in Rust.
using System;
using System.IO;
using System.Text;
using System.Linq;
using System.Diagnostics;
using System.ComponentModel;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.Win32;
using Microsoft.Win32.SafeHandles;

public static class TowavueReleaseHandoff {
    const string PublicKeyHex = "@PUBLIC_KEY_HEX@";
    const string RegistryPath = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue";
    const string StateHeader = "towavue-update-state-v1\n";
    static readonly UTF8Encoding Utf8 = new UTF8Encoding(false, true);

    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
        IntPtr security, uint creation, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInfoNative info);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool MoveFileExW(string source, string target, uint flags);
    [StructLayout(LayoutKind.Sequential)]
    struct FileInfoNative {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Created, Accessed, Written;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }

    static SafeFileHandle Open(string path, bool directory, uint access, uint share, uint creation) {
        var handle = CreateFileW(path, access, share, IntPtr.Zero, creation,
            0x00200000u | (directory ? 0x02000000u : 0u), IntPtr.Zero);
        if (handle.IsInvalid) { int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error); }
        FileInfoNative info;
        if (!GetFileInformationByHandle(handle, out info)) {
            int error = Marshal.GetLastWin32Error(); handle.Dispose(); throw new Win32Exception(error);
        }
        if ((info.Attributes & 0x400u) != 0 || ((info.Attributes & 0x10u) != 0) != directory) {
            handle.Dispose(); throw new IOException("Update path is a link or has the wrong file type.");
        }
        return handle;
    }

    static string LocalPath(string path) {
        if (path == null || path.Length < 4 || !Char.IsLetter(path[0]) || path[1] != ':' || path[2] != '\\'
            || path.Any(c => c < 32 || c == '"') || !String.Equals(Path.GetFullPath(path), path, StringComparison.Ordinal))
            throw new IOException("A normalized local absolute path is required.");
        return path;
    }

    sealed class Directories : IDisposable {
        readonly List<SafeFileHandle> handles = new List<SafeFileHandle>();
        public Directories(string path) {
            LocalPath(path);
            var parts = new Stack<string>();
            for (string part = path; part != null; part = Path.GetDirectoryName(part)) parts.Push(part);
            try { foreach (string part in parts) handles.Add(Open(part, true, 0x80u, 3u, 3u)); }
            catch { Dispose(); throw; }
        }
        public void Dispose() { foreach (var handle in handles) handle.Dispose(); handles.Clear(); }
    }

    static FileStream Read(string path) { return new FileStream(Open(path, false, 0x80000000u, 1u, 3u), FileAccess.Read); }
    static FileStream Mutex(string path) { return new FileStream(Open(path, false, 0xc0000000u, 0u, 4u), FileAccess.ReadWrite); }
    static byte[] Bounded(string path, int maximum) {
        using (var file = Read(path)) {
            if (file.Length > maximum) throw new IOException("Update metadata exceeds its limit.");
            var bytes = new byte[(int)file.Length];
            int at = 0;
            while (at < bytes.Length) { int n = file.Read(bytes, at, bytes.Length - at); if (n == 0) throw new EndOfStreamException(); at += n; }
            return bytes;
        }
    }
    static void WriteNew(string path, byte[] bytes) {
        using (var file = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None)) {
            file.Write(bytes, 0, bytes.Length); file.Flush(true);
        }
    }
    static void State(string root, string stage, string phase) {
        using (var lease = Mutex(Path.Combine(root, "cache.lock"))) {
            string expected = StateHeader + stage + "\ninstalling\n";
            if (Utf8.GetString(Bounded(Path.Combine(root, "state.txt"), 256)) != expected)
                throw new IOException("The selected update state has changed.");
            if (phase == "installing") return;
            string temporary = Path.Combine(root, Guid.NewGuid().ToString("N") + ".state");
            WriteNew(temporary, Utf8.GetBytes(StateHeader + stage + "\n" + phase + "\n"));
            if (!MoveFileExW(temporary, Path.Combine(root, "state.txt"), 9u))
                throw new Win32Exception(Marshal.GetLastWin32Error());
        }
    }
    static byte[] Hex(string text) {
        if ((text.Length & 1) != 0 || text.Any(c => !(c >= '0' && c <= '9') && !(c >= 'a' && c <= 'f')))
            throw new IOException("Invalid hexadecimal metadata.");
        var bytes = new byte[text.Length / 2];
        for (int i = 0; i < bytes.Length; ++i) bytes[i] = Convert.ToByte(text.Substring(i * 2, 2), 16);
        return bytes;
    }
    static Version Stable(string value) {
        var parts = value.Split('.');
        if (parts.Length != 3 || parts.Any(p => p.Length == 0 || (p.Length > 1 && p[0] == '0') || p.Any(c => c < '0' || c > '9')))
            throw new IOException("Invalid stable version.");
        return new Version(value);
    }
    static string Verify(string directory, FileStream setup) {
        byte[] bytes = Bounded(Path.Combine(directory, "manifest.txt"), 256);
        byte[] signature = Bounded(Path.Combine(directory, "manifest.sig"), 512);
        if (signature.Length != 512) throw new IOException("Invalid signature size.");
        using (var key = CngKey.Import(Hex(PublicKeyHex), CngKeyBlobFormat.GenericPublicBlob))
        using (var rsa = new RSACng(key)) {
            if (rsa.KeySize != 4096 || !rsa.VerifyData(bytes, signature, HashAlgorithmName.SHA256, RSASignaturePadding.Pkcs1))
                throw new IOException("Update signature verification failed.");
        }
        string[] lines = Utf8.GetString(bytes).Split('\n');
        if (lines.Length != 6 || lines[0] != "towavue-update-v1" || lines[2] != "windows-x64" || lines[5] != "")
            throw new IOException("Invalid update manifest.");
        Stable(lines[1]);
        ulong size;
        if (!UInt64.TryParse(lines[3], out size) || size < 1 || size > 536870912 || size.ToString() != lines[3]
            || (ulong)setup.Length != size || lines[4].Length != 64)
            throw new IOException("Update size does not match the manifest.");
        using (var hash = SHA256.Create()) {
            setup.Position = 0;
            if (!hash.ComputeHash(setup).SequenceEqual(Hex(lines[4]))) throw new IOException("Update payload hash mismatch.");
        }
        return lines[1];
    }

    static string InstalledVersion(string directory) {
        using (var user = RegistryKey.OpenBaseKey(RegistryHive.CurrentUser, RegistryView.Registry64))
        using (var key = user.OpenSubKey(RegistryPath)) {
            if (key == null || key.GetValueKind("InstallLocation") != RegistryValueKind.String
                || !String.Equals((string)key.GetValue("InstallLocation"), directory, StringComparison.OrdinalIgnoreCase)
                || key.GetValueKind("DisplayVersion") != RegistryValueKind.String
                || key.GetValueNames().Contains("TowavuePendingUpdate"))
                throw new IOException("No matching, complete production installation was found.");
            string version = (string)key.GetValue("DisplayVersion"); Stable(version); return version;
        }
    }
    static string Quote(string value) {
        // Windows CommandLineToArgv/CRT quoting, not PowerShell or cmd syntax.
        var result = new StringBuilder("\""); int slashes = 0;
        foreach (char c in value) {
            if (c == '\\') { ++slashes; continue; }
            if (c == '"') result.Append('\\', slashes * 2 + 1);
            else result.Append('\\', slashes);
            result.Append(c); slashes = 0;
        }
        return result.Append('\\', slashes * 2).Append('"').ToString();
    }

    public static int Run(string root, string stage, string attempt, string installation,
        int parentId, long parentStart, string initialPath) {
        bool claimed = false, parentExited = false, completed = false, restartAllowed = true;
        string directory = null;
        try {
            LocalPath(root); LocalPath(installation);
            if (!System.Text.RegularExpressions.Regex.IsMatch(stage, "\\Astage-[0-9a-f]{56}\\z")
                || !System.Text.RegularExpressions.Regex.IsMatch(attempt, "\\Astage-[0-9a-f]{56}\\z"))
                throw new IOException("Invalid update generation.");
            directory = Path.Combine(root, stage);
            using (var dirs = new Directories(directory))
            using (var installDirs = new Directories(installation))
            using (var handoff = Mutex(Path.Combine(root, "handoff.lock"))) {
                claimed = true;
                try {
                    State(root, stage, "installing");
                    using (var setup = Read(Path.Combine(directory, "setup.exe")))
                    using (var parent = Process.GetProcessById(parentId)) {
                        // Retain the actual process handle, so PID reuse cannot
                        // satisfy the wait. The caller remains alive until ready.
                        IntPtr processHandle = parent.Handle;
                        string executable = Path.Combine(installation, "towavue.exe");
                        if (parent.StartTime.ToUniversalTime().ToFileTimeUtc() != parentStart
                            || !String.Equals(parent.MainModule.FileName, executable, StringComparison.OrdinalIgnoreCase))
                            throw new IOException("The update parent process identity differs.");
                        string version = Verify(directory, setup);
                        if (Stable(version) <= Stable(InstalledVersion(installation))) throw new IOException("The update is not newer.");
                        WriteNew(Path.Combine(directory, attempt + ".ready"), Utf8.GetBytes("ready\n"));
                        if (!parent.WaitForExit(120000)) throw new IOException("Application shutdown timed out; update was not started.");
                        parentExited = true;
                        State(root, stage, "installing");
                        // /D must be last and unquoted, including spaces (NSIS).
                        var start = new ProcessStartInfo(Path.Combine(directory, "setup.exe"), "/S /TOWAVUEUPDATE=1 /TOWAVUEPROGRESS=1 /D=" + installation) {
                            UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = directory
                        };
                        using (var installer = Process.Start(start)) {
                            // Do not kill a running installer midway through its
                            // journaled transaction. New hosts see handoff.lock.
                            installer.WaitForExit();
                            if (installer.ExitCode == 3010) restartAllowed = false;
                            if (installer.ExitCode != 0) throw new IOException("Setup exited with code " + installer.ExitCode + ".");
                        }
                        if (InstalledVersion(installation) != version
                            || FileVersionInfo.GetVersionInfo(executable).ProductVersion != version)
                            throw new IOException("Setup did not install the expected version.");
                        completed = true;
                    }
                } catch (Exception error) {
                    try { State(root, stage, "failed"); } catch { }
                    WriteNew(Path.Combine(directory, attempt + ".error"), Utf8.GetBytes(error.Message));
                    // Recovery belongs to Setup; never perform an implicit
                    // rollback or retry a failed transaction from this helper.
                }
            }
            if (parentExited && restartAllowed) {
                // Release cache/install directory leases before restarting.
                // An incomplete transaction can make this launch unavailable;
                // preserve its error and recovery data for a manual Setup run.
                bool mediaExists = !String.IsNullOrEmpty(initialPath) && (File.Exists(initialPath) || Directory.Exists(initialPath));
                var start = new ProcessStartInfo(Path.Combine(installation, "towavue.exe"), mediaExists ? Quote(initialPath) : "") {
                    UseShellExecute = false, WorkingDirectory = installation
                };
                using (var restarted = Process.Start(start)) { }
            }
            return completed ? 0 : 20;
        } catch (Exception error) {
            // Failure before claiming handoff.lock cannot change another
            // helper's state. The parent detects exit before the ready message.
            Trace.WriteLine((claimed ? "Update helper: " : "Update handoff: ") + error.Message);
            return 21;
        }
    }
}
