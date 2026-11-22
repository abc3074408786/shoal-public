import {ButtonHTMLAttributes,ComponentPropsWithoutRef,forwardRef,ReactNode,useEffect,useId} from 'react';
import * as Tooltip from '@radix-ui/react-tooltip';
import * as Dialog from '@radix-ui/react-dialog';
import {X} from 'lucide-react';

export const Button=forwardRef<HTMLButtonElement,ButtonHTMLAttributes<HTMLButtonElement>>(function Button({className='',...props},ref){return <button ref={ref} className={`ui-button ${className}`} {...props}/>});
export const Input=forwardRef<HTMLInputElement,ComponentPropsWithoutRef<'input'>>(function Input(props,ref){return <input ref={ref} {...props}/>});
export const Textarea=forwardRef<HTMLTextAreaElement,ComponentPropsWithoutRef<'textarea'>>(function Textarea(props,ref){return <textarea ref={ref} {...props}/>});
export function Tip({label,children}:{label:string;children:ReactNode}){return <Tooltip.Root><Tooltip.Trigger asChild>{children}</Tooltip.Trigger><Tooltip.Portal><Tooltip.Content className="ui-tooltip" sideOffset={5}>{label}</Tooltip.Content></Tooltip.Portal></Tooltip.Root>}
export function TooltipProvider({children}:{children:ReactNode}){return <Tooltip.Provider delayDuration={400}>{children}</Tooltip.Provider>}
export function Modal({open,onOpenChange,title,children,className='',closeLabel='关闭对话框'}:{open:boolean;onOpenChange:(open:boolean)=>void;title:string;children:ReactNode;className?:string;closeLabel?:string}){
  return <Dialog.Root open={open} onOpenChange={onOpenChange}><Dialog.Portal><Dialog.Overlay className="ui-overlay"/><Dialog.Content className={'ui-dialog '+className} aria-describedby={undefined}><div className="dialog-heading"><Dialog.Title>{title}</Dialog.Title><Dialog.Close asChild><Button aria-label={closeLabel}><X size={16}/></Button></Dialog.Close></div>{children}</Dialog.Content></Dialog.Portal></Dialog.Root>;
}
/** Existing imperative dialogs share styling and native focus/escape semantics. */
export function NativeDialog({children,...props}:ComponentPropsWithoutRef<'dialog'> & {ref?:React.Ref<HTMLDialogElement>}){const id=useId();return <dialog {...props} data-ui-dialog={id}>{children}</dialog>}
