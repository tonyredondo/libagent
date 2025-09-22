using System;
using System.Runtime.InteropServices;

// Note: adjust the DllImport library name if needed ("libagent" on Unix typically resolves to libagent.so/.dylib)
static class Native
{
    [StructLayout(LayoutKind.Sequential)]
    public struct LibagentHttpBuffer
    {
        public IntPtr data;
        public UIntPtr len;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct LibagentHttpResponse
    {
        public ushort status;
        public LibagentHttpBuffer headers;
        public LibagentHttpBuffer body;
    }

    [DllImport("libagent", EntryPoint = "ProxyTraceAgentUds", CharSet = CharSet.Ansi)]
    public static extern int ProxyTraceAgentUds(
        string method,
        string path,
        string headers,
        IntPtr bodyPtr,
        UIntPtr bodyLen,
        out IntPtr outResp,
        out IntPtr outErr);

    [DllImport("libagent", EntryPoint = "FreeHttpResponse")]
    public static extern void FreeHttpResponse(IntPtr resp);

    [DllImport("libagent", EntryPoint = "FreeCString")]
    public static extern void FreeCString(IntPtr s);
}

class Program
{
    static void Main()
    {
        IntPtr respPtr;
        IntPtr errPtr;
        int rc = Native.ProxyTraceAgentUds("GET", "/info", "Accept: application/json\n", IntPtr.Zero, UIntPtr.Zero, out respPtr, out errPtr);
        if (rc != 0)
        {
            if (errPtr != IntPtr.Zero)
            {
                string err = Marshal.PtrToStringAnsi(errPtr) ?? "(null)";
                Console.Error.WriteLine($"error: {err}");
                Native.FreeCString(errPtr);
            }
            else
            {
                Console.Error.WriteLine($"error rc={rc}");
            }
            return;
        }
        var resp = Marshal.PtrToStructure<Native.LibagentHttpResponse>(respPtr);
        Console.WriteLine($"Status: {resp.status}");
        if (resp.headers.data != IntPtr.Zero && resp.headers.len != UIntPtr.Zero)
        {
            int n = (int)resp.headers.len;
            byte[] hdr = new byte[n];
            Marshal.Copy(resp.headers.data, hdr, 0, n);
            Console.WriteLine("Headers:\n" + System.Text.Encoding.UTF8.GetString(hdr));
        }
        if (resp.body.data != IntPtr.Zero && resp.body.len != UIntPtr.Zero)
        {
            int n = (int)resp.body.len;
            byte[] body = new byte[n];
            Marshal.Copy(resp.body.data, body, 0, n);
            Console.WriteLine("Body:\n" + System.Text.Encoding.UTF8.GetString(body));
        }
        Native.FreeHttpResponse(respPtr);
    }
}

