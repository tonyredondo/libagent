using System;
using System.Runtime.InteropServices;
using System.Threading.Tasks;

// Note: adjust the DllImport library name if needed ("libagent" on Unix typically resolves to libagent.so/.dylib)
static class Native
{
    // Callback delegates
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void ResponseCallback(
        ushort status,
        IntPtr headersData, UIntPtr headersLen,
        IntPtr bodyData, UIntPtr bodyLen,
        IntPtr userData);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    public delegate void ErrorCallback(IntPtr errorMessage, IntPtr userData);

    [DllImport("libagent", EntryPoint = "ProxyTraceAgent", CharSet = CharSet.Ansi)]
    public static extern int ProxyTraceAgent(
        string method,
        string path,
        string headers,
        IntPtr bodyPtr,
        UIntPtr bodyLen,
        ResponseCallback onResponse,
        ErrorCallback onError,
        IntPtr userData);
}

// High-level async API wrapper
public class LibAgentClient
{
    // Response data structure
    public class Response
    {
        public ushort Status { get; }
        public string Headers { get; }
        public byte[] Body { get; }

        public Response(ushort status, string headers, byte[] body)
        {
            Status = status;
            Headers = headers;
            Body = body;
        }
    }

    // Exception for API errors
    public class LibAgentException : Exception
    {
        public int ErrorCode { get; }

        public LibAgentException(string message, int errorCode)
            : base(message)
        {
            ErrorCode = errorCode;
        }
    }

    // Context for async operations
    private class AsyncContext
    {
        public TaskCompletionSource<Response> TaskCompletionSource { get; } = new();
        public Exception? Exception { get; set; }
    }

    // Callback implementations for async operations
    private static readonly ResponseCallback _responseCallback = OnResponse;
    private static readonly ErrorCallback _errorCallback = OnError;

    private static void OnResponse(ushort status,
                                  IntPtr headersData, UIntPtr headersLen,
                                  IntPtr bodyData, UIntPtr bodyLen,
                                  IntPtr userData)
    {
        var context = (AsyncContext)GCHandle.FromIntPtr(userData).Target!;

        try
        {
            // Extract headers
            string headers = "";
            if (headersData != IntPtr.Zero && headersLen != UIntPtr.Zero)
            {
                int headerLen = (int)headersLen;
                byte[] headerBytes = new byte[headerLen];
                Marshal.Copy(headersData, headerBytes, 0, headerLen);
                headers = System.Text.Encoding.UTF8.GetString(headerBytes);
            }

            // Extract body
            byte[] body = Array.Empty<byte>();
            if (bodyData != IntPtr.Zero && bodyLen != UIntPtr.Zero)
            {
                int bodyLenInt = (int)bodyLen;
                body = new byte[bodyLenInt];
                Marshal.Copy(bodyData, body, 0, bodyLenInt);
            }

            var response = new Response(status, headers, body);
            context.TaskCompletionSource.SetResult(response);
        }
        catch (Exception ex)
        {
            context.Exception = ex;
            context.TaskCompletionSource.SetException(ex);
        }
    }

    private static void OnError(IntPtr errorMessage, IntPtr userData)
    {
        var context = (AsyncContext)GCHandle.FromIntPtr(userData).Target!;

        string error = errorMessage != IntPtr.Zero
            ? Marshal.PtrToStringAnsi(errorMessage) ?? "Unknown error"
            : "Unknown error";

        var exception = new LibAgentException(error, -1);
        context.TaskCompletionSource.SetException(exception);
    }

    // High-level async API
    public static async Task<Response> RequestAsync(
        string method,
        string path,
        string headers,
        byte[]? body = null,
        CancellationToken cancellationToken = default)
    {
        // Create context for this request
        var context = new AsyncContext();
        var handle = GCHandle.Alloc(context);

        try
        {
            // Prepare body data
            IntPtr bodyPtr = IntPtr.Zero;
            UIntPtr bodyLen = UIntPtr.Zero;

            if (body != null && body.Length > 0)
            {
                bodyPtr = Marshal.AllocHGlobal(body.Length);
                Marshal.Copy(body, 0, bodyPtr, body.Length);
                bodyLen = new UIntPtr((uint)body.Length);
            }

            // Run the FFI call on a background thread to make it truly async
            await Task.Run(() =>
            {
                try
                {
                    int result = Native.ProxyTraceAgent(
                        method,
                        path,
                        headers,
                        bodyPtr,
                        bodyLen,
                        _responseCallback,
                        _errorCallback,
                        GCHandle.ToIntPtr(handle)
                    );

                    // If the call returned an error code but no exception was set,
                    // create a generic exception
                    if (result != 0 && context.TaskCompletionSource.Task.Status != TaskStatus.Faulted)
                    {
                        var exception = new LibAgentException($"ProxyTraceAgent returned error code {result}", result);
                        context.TaskCompletionSource.SetException(exception);
                    }
                }
                catch (Exception ex)
                {
                    context.TaskCompletionSource.SetException(ex);
                }
                finally
                {
                    // Clean up allocated memory
                    if (bodyPtr != IntPtr.Zero)
                    {
                        Marshal.FreeHGlobal(bodyPtr);
                    }
                }
            }, cancellationToken);

            return await context.TaskCompletionSource.Task;
        }
        finally
        {
            handle.Free();
        }
    }

    // Convenience methods
    public static Task<Response> GetAsync(string path, string headers = "", CancellationToken cancellationToken = default)
        => RequestAsync("GET", path, headers, null, cancellationToken);

    public static Task<Response> PostAsync(string path, string headers = "", byte[]? body = null, CancellationToken cancellationToken = default)
        => RequestAsync("POST", path, headers, body, cancellationToken);

    public static Task<Response> PutAsync(string path, string headers = "", byte[]? body = null, CancellationToken cancellationToken = default)
        => RequestAsync("PUT", path, headers, body, cancellationToken);

    public static Task<Response> DeleteAsync(string path, string headers = "", CancellationToken cancellationToken = default)
        => RequestAsync("DELETE", path, headers, body: null, cancellationToken);
}

class Program
{
    static async Task Main()
    {
        try
        {
            // Example 1: Simple GET request
            Console.WriteLine("=== Example 1: Simple GET ===");
            var response = await LibAgentClient.GetAsync("/info", "Accept: application/json\n");
            Console.WriteLine($"Status: {response.Status}");
            Console.WriteLine($"Headers: {response.Headers}");
            Console.WriteLine($"Body: {System.Text.Encoding.UTF8.GetString(response.Body)}");

            // Example 2: POST request with body
            Console.WriteLine("\n=== Example 2: POST with body ===");
            byte[] postBody = System.Text.Encoding.UTF8.GetBytes("{\"key\": \"value\"}");
            var postResponse = await LibAgentClient.PostAsync(
                "/api/test",
                "Content-Type: application/json\nAccept: application/json\n",
                postBody);
            Console.WriteLine($"POST Status: {postResponse.Status}");

            // Example 3: Using the generic RequestAsync method
            Console.WriteLine("\n=== Example 3: Generic request ===");
            var customResponse = await LibAgentClient.RequestAsync(
                "PUT",
                "/api/config",
                "Content-Type: application/json\n",
                System.Text.Encoding.UTF8.GetBytes("{\"setting\": true}"));
            Console.WriteLine($"PUT Status: {customResponse.Status}");
        }
        catch (LibAgentClient.LibAgentException ex)
        {
            Console.Error.WriteLine($"API Error (Code: {ex.ErrorCode}): {ex.Message}");
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"Unexpected error: {ex.Message}");
        }
    }

    // Alternative: Low-level callback API (still available)
    static void LowLevelExample()
    {
        // This shows the callback-based API still works
        Native.ResponseCallback responseCallback = (status, headersData, headersLen, bodyData, bodyLen, userData) =>
        {
            Console.WriteLine($"Low-level Status: {status}");
            // ... callback implementation ...
        };

        Native.ErrorCallback errorCallback = (errorMessage, userData) =>
        {
            string error = errorMessage != IntPtr.Zero
                ? Marshal.PtrToStringAnsi(errorMessage) ?? "Unknown error"
                : "Unknown error";
            Console.Error.WriteLine($"Low-level error: {error}");
        };

        int rc = Native.ProxyTraceAgent(
            "GET", "/info", "Accept: application/json\n",
            IntPtr.Zero, UIntPtr.Zero,
            responseCallback, errorCallback, IntPtr.Zero);

        Console.WriteLine($"Low-level return code: {rc}");
    }
}
