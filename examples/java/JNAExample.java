package examples.java;

import com.sun.jna.*;
import com.sun.jna.ptr.PointerByReference;

public class JNAExample {
    public static class LibagentHttpBuffer extends Structure {
        public Pointer data;
        public NativeLong len; // size_t
        @Override
        protected java.util.List<String> getFieldOrder() {
            return java.util.Arrays.asList("data", "len");
        }
    }

    public static class LibagentHttpResponse extends Structure {
        public short status; // uint16_t
        public LibagentHttpBuffer headers;
        public LibagentHttpBuffer body;
        @Override
        protected java.util.List<String> getFieldOrder() {
            return java.util.Arrays.asList("status", "headers", "body");
        }
    }

    public interface LibAgent extends Library {
        LibAgent INSTANCE = Native.load("agent", LibAgent.class);

        int ProxyTraceAgentUds(String method,
                               String path,
                               String headers,
                               Pointer bodyPtr,
                               NativeLong bodyLen,
                               PointerByReference outResp,
                               PointerByReference outErr);

        void FreeHttpResponse(Pointer resp);
        void FreeCString(Pointer s);
    }

    public static void main(String[] args) {
        PointerByReference outResp = new PointerByReference();
        PointerByReference outErr = new PointerByReference();
        int rc = LibAgent.INSTANCE.ProxyTraceAgentUds(
                "GET",
                "/info",
                "Accept: application/json\n",
                Pointer.NULL,
                new NativeLong(0),
                outResp,
                outErr
        );
        if (rc != 0) {
            Pointer err = outErr.getValue();
            if (err != null) {
                System.err.println("error: " + err.getString(0));
                LibAgent.INSTANCE.FreeCString(err);
            } else {
                System.err.println("error: rc=" + rc);
            }
            return;
        }
        Pointer p = outResp.getValue();
        LibagentHttpResponse resp = new LibagentHttpResponse(p);
        resp.read();
        System.out.println("Status: " + (resp.status & 0xFFFF));
        String headers = resp.headers.data.getString(0);
        byte[] body = resp.body.data.getByteArray(0, resp.body.len.intValue());
        System.out.println("Headers:\n" + headers);
        System.out.println("Body:\n" + new String(body));
        LibAgent.INSTANCE.FreeHttpResponse(p);
    }
}

