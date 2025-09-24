package examples.java;

import com.sun.jna.*;

public class JNAExample {
    // Callback interfaces
    public interface ResponseCallback extends Callback {
        void invoke(short status,
                   Pointer headersData, NativeLong headersLen,
                   Pointer bodyData, NativeLong bodyLen,
                   Pointer userData);
    }

    public interface ErrorCallback extends Callback {
        void invoke(Pointer errorMessage, Pointer userData);
    }

    public interface LibAgent extends Library {
        LibAgent INSTANCE = Native.load("agent", LibAgent.class);

        int ProxyTraceAgent(String method,
                           String path,
                           String headers,
                           Pointer bodyPtr,
                           NativeLong bodyLen,
                           ResponseCallback onResponse,
                           ErrorCallback onError,
                           Pointer userData);
    }

    // Callback implementations
    public static class ResponseCallbackImpl implements ResponseCallback {
        @Override
        public void invoke(short status,
                          Pointer headersData, NativeLong headersLen,
                          Pointer bodyData, NativeLong bodyLen,
                          Pointer userData) {
            System.out.println("Status: " + (status & 0xFFFF));
            if (headersData != null && headersLen.intValue() > 0) {
                String headers = headersData.getString(0);
                System.out.println("Headers:\n" + headers);
            }
            if (bodyData != null && bodyLen.intValue() > 0) {
                byte[] body = bodyData.getByteArray(0, bodyLen.intValue());
                System.out.println("Body:\n" + new String(body));
            }
        }
    }

    public static class ErrorCallbackImpl implements ErrorCallback {
        @Override
        public void invoke(Pointer errorMessage, Pointer userData) {
            if (errorMessage != null) {
                System.err.println("error: " + errorMessage.getString(0));
            } else {
                System.err.println("error: unknown error");
            }
        }
    }

    public static void main(String[] args) {
        // Create callback instances
        ResponseCallbackImpl responseCallback = new ResponseCallbackImpl();
        ErrorCallbackImpl errorCallback = new ErrorCallbackImpl();

        // Make the request using callbacks - no manual memory management!
        int rc = LibAgent.INSTANCE.ProxyTraceAgent(
                "GET",                           // method
                "/info",                         // path
                "Accept: application/json\n",    // headers
                Pointer.NULL,                    // body (none)
                new NativeLong(0),               // body length
                responseCallback,                // success callback
                errorCallback,                   // error callback
                Pointer.NULL                     // user data (not used)
        );

        if (rc != 0) {
            System.err.println("ProxyTraceAgent returned error code: " + rc);
        }
    }
}
