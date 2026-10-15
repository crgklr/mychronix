/* aimhook: a stand-in msimg32.dll for RaceStudio 2 under Wine.
 *
 * Forwards msimg32's three exports to the real one and, when loaded,
 * replaces RS2's imported DeviceIoControl. Requests to AiM's USB driver
 * (IOCTL 0x22000c / 0x220010) carry a pointer to the command bytes, which
 * Wine's out-of-process driver can't follow; here they're read in RS2's own
 * process and either logged (AIMHOOK=log) or sent to bridge.py, which does
 * the USB transfer.
 *
 * Build: i686-w64-mingw32-gcc -shared -O2 -o msimg32.dll hook.c msimg32.def -lws2_32
 */
#include <winsock2.h>
#include <windows.h>
#include <stdio.h>

static HMODULE real;
static FILE *logf;
typedef BOOL(WINAPI *DIOC)(HANDLE, DWORD, LPVOID, DWORD, LPVOID, DWORD, LPDWORD, LPOVERLAPPED);
static DIOC orig_dioc;
static SOCKET sock = INVALID_SOCKET;
static int bridge;

static void hexdump(const char *what, const void *p, DWORD n) {
    const unsigned char *b = p;
    fprintf(logf, "%s (%lu):", what, n);
    for (DWORD i = 0; i < n; i++) fprintf(logf, "%s%02x", i % 8 ? "" : " ", b[i]);
    fprintf(logf, "\n");
}

/* Dumps bytes at each aligned 32-bit value that points into readable memory. */
static void dump_pointers(const unsigned char *rec, DWORD n) {
    for (DWORD off = 0; off + 4 <= n; off += 4) {
        DWORD v = *(const DWORD *)(rec + off);
        if (v < 0x10000) continue;
        MEMORY_BASIC_INFORMATION mi;
        if (!VirtualQuery((void *)v, &mi, sizeof mi) || mi.State != MEM_COMMIT || (mi.Protect & (PAGE_NOACCESS | PAGE_GUARD))) continue;
        char what[32];
        sprintf(what, "  *[+0x%02lx]=%08lx", off, v);
        hexdump(what, (void *)v, 16);
    }
}

static int recv_all(char *p, int n) {
    while (n > 0) {
        int r = recv(sock, p, n, 0);
        if (r <= 0) return 0;
        p += r; n -= r;
    }
    return 1;
}

static int bridge_open(void) {
    if (sock != INVALID_SOCKET) return 1;
    WSADATA w;
    WSAStartup(MAKEWORD(2, 2), &w);
    sock = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
    struct sockaddr_in a = {0};
    a.sin_family = AF_INET;
    a.sin_port = htons(47321);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    if (connect(sock, (struct sockaddr *)&a, sizeof a)) {
        closesocket(sock);
        sock = INVALID_SOCKET;
        if (logf) fprintf(logf, "bridge not running\n");
        return 0;
    }
    return 1;
}

/* One transfer through bridge.py; returns bytes transferred or a negative error. */
static int bridge_xfer(DWORD kind, DWORD ep, const BYTE setup[8], BYTE *buf, DWORD n, int is_in) {
    if (!bridge_open()) return -1;
    BYTE hdr[24];
    DWORD v[3] = {kind, ep, 5000};
    memcpy(hdr, v, 12);
    memcpy(hdr + 12, setup ? setup : (const BYTE *)"\0\0\0\0\0\0\0\0", 8);
    memcpy(hdr + 20, &n, 4);
    if (send(sock, (char *)hdr, 24, 0) != 24) goto fail;
    if (!is_in && n && send(sock, (char *)buf, n, 0) != (int)n) goto fail;
    int r;
    if (!recv_all((char *)&r, 4)) goto fail;
    if (is_in && r > 0 && !recv_all((char *)buf, r)) goto fail;
    return r;
fail:
    closesocket(sock);
    sock = INVALID_SOCKET;
    return -1;
}

static BOOL WINAPI hook_dioc(HANDLE h, DWORD code, LPVOID in, DWORD inn, LPVOID out, DWORD outn, LPDWORD ret, LPOVERLAPPED ov) {
    int aim = (code == 0x0022000c || code == 0x00220010) && in && inn == 56 && out && outn >= 56;
    if ((code & 0xffff0000) == 0x00220000 && logf) {
        fprintf(logf, "IOCTL %08lx in=%lu out=%lu\n", code, inn, outn);
        if (in && inn) { hexdump(" in", in, inn); dump_pointers(in, inn); }
        fflush(logf);
    }
    if (aim && bridge) {
        BYTE rec[56];
        memcpy(rec, in, 56);
        BYTE *buf = (BYTE *)(ULONG_PTR) * (DWORD *)(rec + 0x10);
        DWORD n = *(DWORD *)(rec + 0x18);
        int r;
        if (code == 0x0022000c) {
            /* Control transfer: setup packet at +0x24, data stage at buf. */
            int is_in = rec[0x24] & 0x80;
            r = bridge_xfer(1, 0, rec + 0x24, buf, n, is_in);
        } else {
            /* Bulk/interrupt transfer: endpoint address at +0x04, read flag at +0x08. */
            DWORD ep = *(DWORD *)(rec + 0x04) & 0xff, rd = *(DWORD *)(rec + 0x08);
            if (!ep) ep = rd ? 0x82 : 0x02;
            r = bridge_xfer(rd ? 3 : 2, ep, NULL, buf, n, rd != 0);
        }
        *(DWORD *)(rec + 0x20) = r > 0 ? r : 0;
        *(DWORD *)(rec + 0x2c) = r >= 0;
        *(DWORD *)(rec + 0x30) = r >= 0 ? 0 : 0xC0000001;
        memcpy(out, rec, 56);
        if (ret) *ret = 56;
        if (logf) {
            fprintf(logf, " bridged -> %d\n", r);
            if (r > 0) hexdump(" data", buf, r > 256 ? 256 : r);
            fflush(logf);
        }
        return TRUE;
    }
    BOOL r = orig_dioc(h, code, in, inn, out, outn, ret, ov);
    if ((code & 0xffff0000) == 0x00220000 && logf) {
        fprintf(logf, " -> %d ret=%lu\n", r, ret ? *ret : 0);
        if (out && outn) hexdump(" out", out, outn);
        fflush(logf);
    }
    return r;
}

static void patch_iat(HMODULE mod) {
    BYTE *base = (BYTE *)mod;
    IMAGE_NT_HEADERS *nt = (IMAGE_NT_HEADERS *)(base + ((IMAGE_DOS_HEADER *)base)->e_lfanew);
    IMAGE_DATA_DIRECTORY dir = nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (!dir.VirtualAddress) return;
    FARPROC target = GetProcAddress(GetModuleHandleA("kernel32.dll"), "DeviceIoControl");
    for (IMAGE_IMPORT_DESCRIPTOR *d = (void *)(base + dir.VirtualAddress); d->Name; d++) {
        if (lstrcmpiA((char *)base + d->Name, "KERNEL32.dll")) continue;
        for (FARPROC *slot = (FARPROC *)(base + d->FirstThunk); *slot; slot++) {
            if (*slot != target) continue;
            DWORD old;
            VirtualProtect(slot, sizeof *slot, PAGE_READWRITE, &old);
            orig_dioc = (DIOC)*slot;
            *slot = (FARPROC)hook_dioc;
            VirtualProtect(slot, sizeof *slot, old, &old);
            if (logf) fprintf(logf, "hooked DeviceIoControl\n");
        }
    }
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD why, LPVOID r) {
    if (why == DLL_PROCESS_ATTACH) {
        char path[MAX_PATH];
        GetSystemDirectoryA(path, MAX_PATH);
        lstrcatA(path, "\\msimg32.dll");
        real = LoadLibraryA(path);
        char exe[MAX_PATH], name[MAX_PATH + 32];
        GetModuleFileNameA(NULL, exe, MAX_PATH);
        sprintf(name, "%s.aimhook.log", exe);
        logf = fopen(name, "a");
        if (logf) fprintf(logf, "=== aimhook loaded in %s\n", exe);
        patch_iat(GetModuleHandleA(NULL));
        if (logf) fflush(logf);
        char mode[16] = "";
        GetEnvironmentVariableA("AIMHOOK", mode, sizeof mode);
        bridge = lstrcmpiA(mode, "log") != 0;
        if (logf) fprintf(logf, "mode: %s\n", bridge ? "bridge" : "log only");
    }
    return TRUE;
}

/* msimg32's exports (named in msimg32.def), forwarded to the real DLL. */
#define FWD(name, params, args)                                                \
    BOOL WINAPI fwd_##name params {                                           \
        static BOOL(WINAPI * fn_) params;                                     \
        if (!fn_) fn_ = (BOOL(WINAPI *) params)GetProcAddress(real, #name);    \
        return fn_ args;                                                      \
    }
FWD(AlphaBlend, (HDC a, int b, int c, int d, int e, HDC f, int g, int h, int i, int j, BLENDFUNCTION k), (a, b, c, d, e, f, g, h, i, j, k))
FWD(GradientFill, (HDC a, PTRIVERTEX b, ULONG c, PVOID d, ULONG e, ULONG f), (a, b, c, d, e, f))
FWD(TransparentBlt, (HDC a, int b, int c, int d, int e, HDC f, int g, int h, int i, int j, UINT k), (a, b, c, d, e, f, g, h, i, j, k))
