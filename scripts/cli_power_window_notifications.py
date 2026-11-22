"""Passive, message-only Windows notification receiver for the power UAT.

It owns no visible window, sends no input and never changes lock/display state.
"""
import ctypes as c
from ctypes import wintypes as w
import queue
import threading
import time
import uuid


class WindowObserver:
    SETTINGS = {'lid': 'ba3e0f4d-b817-4094-a2d1-d56379e6a0f3',
                'display': '2b84c20e-ad23-4ddf-93db-05ffbd7efca5',
                'console_display': '6fe69556-704a-47a0-8f24-c28d936fda47',
                'source': '5d3e9a59-e9d5-4b00-a6bd-ff34ff516548'}

    def __init__(self):
        self.events, self.ready, self.ping = queue.SimpleQueue(), queue.Queue(), threading.Event()
        self.hwnd, self.error = None, None
        self.thread = threading.Thread(target=self._run, name='power-uat-notifications', daemon=True)
        self.thread.start()
        status = self.ready.get(timeout=5)
        if status is not None:
            self.thread.join(timeout=5)
            raise status
        # Test the owned queue/message loop, not a fabricated OS power/lock event.
        if not self.user.PostMessageW(self.hwnd, 0x8033, 0, 0) or not self.ping.wait(3):
            self.close()
            raise RuntimeError('Notification message loop did not answer its private ping')

    def _run(self):
        self.user = c.WinDLL('user32.dll', use_last_error=True)
        kernel = c.WinDLL('kernel32.dll', use_last_error=True)
        wts = c.WinDLL('wtsapi32.dll', use_last_error=True)
        result_type = c.c_ssize_t
        proc_type = c.WINFUNCTYPE(result_type, c.c_void_p, w.UINT, c.c_size_t, c.c_ssize_t)

        class WindowClass(c.Structure):
            _fields_ = [('style', w.UINT), ('proc', proc_type), ('class_extra', c.c_int),
                        ('window_extra', c.c_int), ('instance', c.c_void_p), ('icon', c.c_void_p),
                        ('cursor', c.c_void_p), ('background', c.c_void_p),
                        ('menu', w.LPCWSTR), ('name', w.LPCWSTR)]

        self.user.DefWindowProcW.argtypes = [c.c_void_p, w.UINT, c.c_size_t, c.c_ssize_t]
        self.user.DefWindowProcW.restype = result_type
        self.user.RegisterClassW.argtypes = [c.POINTER(WindowClass)]
        self.user.RegisterClassW.restype = w.ATOM
        self.user.UnregisterClassW.argtypes = [w.LPCWSTR, c.c_void_p]
        self.user.CreateWindowExW.argtypes = [w.DWORD, w.LPCWSTR, w.LPCWSTR, w.DWORD,
                                            c.c_int, c.c_int, c.c_int, c.c_int,
                                            c.c_void_p, c.c_void_p, c.c_void_p, c.c_void_p]
        self.user.CreateWindowExW.restype = c.c_void_p
        self.user.DestroyWindow.argtypes = [c.c_void_p]
        self.user.GetMessageW.argtypes = [c.POINTER(w.MSG), c.c_void_p, w.UINT, w.UINT]
        self.user.GetMessageW.restype = w.BOOL
        self.user.DispatchMessageW.argtypes = [c.POINTER(w.MSG)]
        self.user.DispatchMessageW.restype = result_type
        self.user.PostMessageW.argtypes = [c.c_void_p, w.UINT, c.c_size_t, c.c_ssize_t]
        self.user.PostMessageW.restype = w.BOOL
        self.user.RegisterPowerSettingNotification.argtypes = [c.c_void_p, c.c_void_p, w.DWORD]
        self.user.RegisterPowerSettingNotification.restype = c.c_void_p
        self.user.UnregisterPowerSettingNotification.argtypes = [c.c_void_p]
        wts.WTSRegisterSessionNotification.argtypes = [c.c_void_p, w.DWORD]
        wts.WTSRegisterSessionNotification.restype = w.BOOL
        wts.WTSUnRegisterSessionNotification.argtypes = [c.c_void_p]
        kernel.GetModuleHandleW.argtypes = [w.LPCWSTR]
        kernel.GetModuleHandleW.restype = c.c_void_p

        def receive(hwnd, message, wp, lp):
            if message == 0x8033:
                self.ping.set()
                return 0
            if message == 0x2b1:
                event = {'wall_ns': time.time_ns(), 'channel': 'window',
                         'kind': 'session', 'code': wp, 'session_id': lp}
                if wp in (7, 8):
                    event.update(setting='lock', value=0 if wp == 7 else 1)
                self.events.put(event)
                return 0
            if message == 0x218 and wp == 0x8013 and lp:
                header = c.string_at(lp, 20)
                guid, size = str(uuid.UUID(bytes_le=header[:16])), int.from_bytes(header[16:], 'little')
                setting = next((key for key, value in self.SETTINGS.items() if value == guid), None)
                if setting and size == 4:
                    self.events.put({'wall_ns': time.time_ns(), 'channel': 'window', 'kind': 'power',
                                     'setting': setting, 'guid': guid,
                                     'value': int.from_bytes(c.string_at(lp + 20, 4), 'little')})
                return 1
            if message == 0x10:
                self.user.PostQuitMessage(0)
                return 0
            return self.user.DefWindowProcW(hwnd, message, wp, lp)

        self.callback = proc_type(receive)
        instance, name = kernel.GetModuleHandleW(None), 'VelellaPowerUat_' + uuid.uuid4().hex
        window_class = WindowClass(proc=self.callback, instance=instance, name=name)
        registered, session_registered, notifications, ready_sent = False, False, [], False
        try:
            if not self.user.RegisterClassW(c.byref(window_class)):
                raise c.WinError(c.get_last_error())
            registered = True
            self.hwnd = self.user.CreateWindowExW(0, name, '', 0, 0, 0, 0, 0, c.c_void_p(-3), None, instance, None)
            if not self.hwnd:
                raise c.WinError(c.get_last_error())
            if not wts.WTSRegisterSessionNotification(self.hwnd, 0):
                raise c.WinError(c.get_last_error())
            session_registered = True
            for guid in self.SETTINGS.values():
                raw = c.create_string_buffer(uuid.UUID(guid).bytes_le)
                handle = self.user.RegisterPowerSettingNotification(self.hwnd, raw, 0)
                if not handle:
                    raise c.WinError(c.get_last_error())
                notifications.append(handle)
            self.ready.put(None)
            ready_sent = True
            message = w.MSG()
            while True:
                status = self.user.GetMessageW(c.byref(message), None, 0, 0)
                if status == -1:
                    raise c.WinError(c.get_last_error())
                if status == 0:
                    break
                self.user.DispatchMessageW(c.byref(message))
        except BaseException as error:
            self.error = error
            if not ready_sent:
                self.ready.put(error)
        finally:
            for handle in notifications:
                self.user.UnregisterPowerSettingNotification(handle)
            if session_registered:
                wts.WTSUnRegisterSessionNotification(self.hwnd)
            if self.hwnd:
                self.user.DestroyWindow(self.hwnd)
            self.hwnd = None
            if registered:
                self.user.UnregisterClassW(name, instance)

    def drain(self):
        if self.error:
            raise self.error
        events = []
        while not self.events.empty():
            events.append(self.events.get())
        return events

    def close(self):
        if self.hwnd:
            self.user.PostMessageW(self.hwnd, 0x10, 0, 0)
        self.thread.join(timeout=5)
        if self.thread.is_alive():
            raise RuntimeError('Notification receiver did not stop')
