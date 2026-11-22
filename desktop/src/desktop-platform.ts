import {
  invoke as tauriInvoke,
  isTauri as nativeTauri,
} from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWindow as tauriWindow } from "@tauri-apps/api/window";
import * as dialogs from "@tauri-apps/plugin-dialog";
import * as notifications from "@tauri-apps/plugin-notification";
type Bridge = {
  invoke<T>(command: string, args?: unknown): Promise<T>;
  listen(event: string, callback: (event: any) => void): () => void;
};
declare global {
  interface Window {
    velellaDesktop?: Bridge;
  }
}
const bridge = () => window.velellaDesktop;
export const isTauri = () => !!bridge() || nativeTauri();
export function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return bridge()
    ? bridge()!.invoke<T>(command, args)
    : tauriInvoke<T>(command, args);
}
export function listen<T>(
  event: string,
  callback: (event: { payload: T }) => void,
): Promise<() => void> {
  return bridge()
    ? Promise.resolve(bridge()!.listen(event, callback))
    : tauriListen<T>(event, callback);
}
export function getCurrentWindow() {
  if (!bridge()) return tauriWindow();
  const action = <T = void>(action: string) => invoke<T>("window", { action });
  return {
    minimize: () => action("minimize"),
    toggleMaximize: () => action("toggleMaximize"),
    close: () => action("close"),
    show: () => action("show"),
    setFocus: () => action("setFocus"),
    isMaximized: () => action<boolean>("isMaximized"),
    onResized: (callback: () => void) => listen("window-resized", callback),
  };
}
export const open: typeof dialogs.open = ((options: any) =>
  bridge()
    ? invoke("dialog_open", { options })
    : dialogs.open(options)) as typeof dialogs.open;
export const confirm: typeof dialogs.confirm = (message, options) =>
  bridge()
    ? invoke<boolean>("dialog_confirm", { message, options })
    : dialogs.confirm(message, options);
export const isPermissionGranted = () =>
  bridge() ? Promise.resolve(true) : notifications.isPermissionGranted();
export const requestPermission = () =>
  bridge()
    ? Promise.resolve("granted" as const)
    : notifications.requestPermission();
export const sendNotification: typeof notifications.sendNotification = (
  options,
) => {
  if (bridge()) void invoke("notify", { options }).catch(console.error);
  else notifications.sendNotification(options);
};
export async function notifyDesktop(options:{title:string;body:string;silent?:boolean;extra?:Record<string,unknown>}){
  if(!isTauri())throw Error('系统通知仅在桌面应用中可用。');
  if(bridge()){await invoke('notify',{options});return;}
  const permission=await isPermissionGranted()||await requestPermission()==='granted';
  if(!permission)throw Error('系统通知权限未开启，请在系统设置中允许通知。');
  notifications.sendNotification(options);
}
export const onAction = async (
  callback: Parameters<typeof notifications.onAction>[0],
) => {
  if (!bridge()) return notifications.onAction(callback);
  const off = await listen<any>("notification-action", (event) =>
    callback(event.payload),
  );
  return { unregister: async () => off() };
};
