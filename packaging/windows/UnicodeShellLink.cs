using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Text;
using System.Threading;

public static class TowavueInstallerShellLink
{
    [ComImport, Guid("000214F9-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IShellLinkW
    {
        void GetPath([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, IntPtr findData, uint flags);
        void GetIDList(out IntPtr pidl);
        void SetIDList(IntPtr pidl);
        void GetDescription([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int size);
        void SetDescription([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetWorkingDirectory([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size);
        void SetWorkingDirectory([MarshalAs(UnmanagedType.LPWStr)] string path);
        void GetArguments([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder text, int size);
        void SetArguments([MarshalAs(UnmanagedType.LPWStr)] string text);
        void GetHotkey(out ushort hotkey);
        void SetHotkey(ushort hotkey);
        void GetShowCmd(out int command);
        void SetShowCmd(int command);
        void GetIconLocation([Out, MarshalAs(UnmanagedType.LPWStr)] StringBuilder path, int size, out int index);
        void SetIconLocation([MarshalAs(UnmanagedType.LPWStr)] string path, int index);
        void SetRelativePath([MarshalAs(UnmanagedType.LPWStr)] string path, uint reserved);
        void Resolve(IntPtr window, uint flags);
        void SetPath([MarshalAs(UnmanagedType.LPWStr)] string path);
    }

    private static object CreateObject()
    {
        if (Thread.CurrentThread.GetApartmentState() != ApartmentState.STA)
            throw new InvalidOperationException("Shell link operations require an STA process.");
        return Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("00021401-0000-0000-C000-000000000046")));
    }

    // The caller preflights a new owned path. All COM references remain on this STA
    // for this synchronous call and are released once; no pointer escapes or target runs.
    public static void Create(string shortcut, string executable, string workingDirectory)
    {
        if (File.Exists(shortcut) || Directory.Exists(shortcut)) throw new IOException("Shortcut already exists.");
        object instance = CreateObject();
        try
        {
            IShellLinkW link = (IShellLinkW)instance;
            link.SetPath(executable);
            link.SetWorkingDirectory(workingDirectory);
            link.SetArguments("");
            link.SetIconLocation(executable, 0);
            link.SetDescription("Open towavue (local evaluation)");
            link.SetShowCmd(1);
            ((IPersistFile)instance).Save(shortcut, true);
        }
        finally { Marshal.FinalReleaseComObject(instance); }
    }

    // Load read-only without Resolve: inspection must not search for or launch a target.
    public static string[] Read(string shortcut)
    {
        object instance = CreateObject();
        try
        {
            ((IPersistFile)instance).Load(shortcut, 0);
            IShellLinkW link = (IShellLinkW)instance;
            StringBuilder path = new StringBuilder(1024), working = new StringBuilder(1024), arguments = new StringBuilder(1024);
            link.GetPath(path, path.Capacity, IntPtr.Zero, 0);
            link.GetWorkingDirectory(working, working.Capacity);
            link.GetArguments(arguments, arguments.Capacity);
            return new string[] { path.ToString(), working.ToString(), arguments.ToString() };
        }
        finally { Marshal.FinalReleaseComObject(instance); }
    }
}
