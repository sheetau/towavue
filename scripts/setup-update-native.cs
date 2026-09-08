using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class TowavueUpdateFiles {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool MoveFileExW(string source, string destination, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CopyFileW(string source, string destination, bool failIfExists);

    // The caller validates local non-reparse paths and retains sharing-restricted
    // handles. These synchronous calls retain no managed pointers or handles.
    // Never copy across volumes, write through a hard link, or schedule a reboot.
    // Private work names can exceed MAX_PATH even when installed names do not.
    // Do not depend on the embedding PowerShell executable's long-path manifest.
    private static string NativePath(string path) {
        return @"\\?\" + System.IO.Path.GetFullPath(path);
    }
    public static void Move(string source, string destination) {
        if (!MoveFileExW(NativePath(source), NativePath(destination), 8u))
            throw new Win32Exception(Marshal.GetLastWin32Error());
    }
    public static void Copy(string source, string destination) {
        if (!CopyFileW(NativePath(source), NativePath(destination), true))
            throw new Win32Exception(Marshal.GetLastWin32Error());
    }
}
