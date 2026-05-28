import {
  ButtonHTMLAttributes,
  createContext,
  HTMLAttributes,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";

type Align = "start" | "end";

type MenuCtx = {
  open: boolean;
  setOpen: (v: boolean) => void;
  toggle: () => void;
  align: Align;
  menuId: string;
  triggerRef: React.RefObject<HTMLButtonElement>;
  listRef: React.RefObject<HTMLDivElement>;
};

const Ctx = createContext<MenuCtx | null>(null);
function useMenu() {
  const ctx = useContext(Ctx);
  if (!ctx) throw new Error("Menu.* must be used within <Menu>");
  return ctx;
}

/**
 * Accessible dropdown menu. Composition:
 *   <Menu>
 *     <MenuButton>…</MenuButton>
 *     <MenuList>
 *       <MenuItem onSelect={…}>…</MenuItem>
 *     </MenuList>
 *   </Menu>
 *
 * Handles Escape (closes + returns focus to trigger), Arrow/Home/End roving,
 * Enter/Space activation, and click-outside. Anchored to the trigger; no portal.
 */
export function Menu({ align = "start", children }: { align?: Align; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const toggle = useCallback(() => setOpen((o) => !o), []);

  // Click outside closes.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      const t = e.target as Node;
      if (listRef.current?.contains(t) || triggerRef.current?.contains(t)) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open]);

  return (
    <Ctx.Provider value={{ open, setOpen, toggle, align, menuId, triggerRef, listRef }}>
      <div className="relative inline-flex">{children}</div>
    </Ctx.Provider>
  );
}

type MenuButtonProps = ButtonHTMLAttributes<HTMLButtonElement>;

export function MenuButton({ children, onClick, ...rest }: MenuButtonProps) {
  const { open, toggle, menuId, triggerRef } = useMenu();
  return (
    <button
      ref={triggerRef}
      type="button"
      aria-haspopup="menu"
      aria-expanded={open}
      aria-controls={open ? menuId : undefined}
      onClick={(e) => {
        onClick?.(e);
        toggle();
      }}
      {...rest}
    >
      {children}
    </button>
  );
}

const ALIGN: Record<Align, string> = {
  start: "left-0 origin-top-left",
  end: "right-0 origin-top-right",
};

type Side = "top" | "bottom";
const SIDE: Record<Side, string> = {
  bottom: "top-full mt-1",
  top: "bottom-full mb-1",
};

export function MenuList({
  side = "bottom",
  className = "",
  children,
  ...rest
}: HTMLAttributes<HTMLDivElement> & { side?: Side }) {
  const { open, setOpen, align, menuId, triggerRef, listRef } = useMenu();

  const items = useCallback(
    () =>
      Array.from(
        listRef.current?.querySelectorAll<HTMLElement>(
          '[role="menuitem"]:not([aria-disabled="true"])',
        ) ?? [],
      ),
    [listRef],
  );

  // Focus the first item when opening.
  useEffect(() => {
    if (!open) return;
    const first = items()[0];
    first?.focus();
  }, [open, items]);

  const close = useCallback(
    (focusTrigger = true) => {
      setOpen(false);
      if (focusTrigger) triggerRef.current?.focus();
    },
    [setOpen, triggerRef],
  );

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const list = items();
    const idx = list.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      list[Math.min(idx + 1, list.length - 1)]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      list[Math.max(idx - 1, 0)]?.focus();
    } else if (e.key === "Home") {
      e.preventDefault();
      list[0]?.focus();
    } else if (e.key === "End") {
      e.preventDefault();
      list[list.length - 1]?.focus();
    } else if (e.key === "Tab") {
      // Menus trap focus by closing on Tab-away.
      close(false);
    }
  };

  if (!open) return null;
  return (
    <div
      ref={listRef}
      id={menuId}
      role="menu"
      tabIndex={-1}
      onKeyDown={onKeyDown}
      className={[
        "absolute z-50 min-w-[10rem]",
        "rounded-lg border border-line bg-surface-elev shadow-lg",
        "p-1",
        SIDE[side],
        ALIGN[align],
        className,
      ].join(" ")}
      {...rest}
    >
      {children}
    </div>
  );
}

type MenuItemProps = Omit<HTMLAttributes<HTMLButtonElement>, "onSelect"> & {
  onSelect?: () => void;
  disabled?: boolean;
  tone?: "default" | "danger";
  leftIcon?: ReactNode;
  active?: boolean;
};

export function MenuItem({
  onSelect,
  disabled,
  tone = "default",
  leftIcon,
  active = false,
  className = "",
  children,
  ...rest
}: MenuItemProps) {
  const { setOpen, triggerRef } = useMenu();
  return (
    <button
      type="button"
      role="menuitem"
      aria-disabled={disabled || undefined}
      disabled={disabled}
      tabIndex={-1}
      onClick={() => {
        if (disabled) return;
        onSelect?.();
        setOpen(false);
        triggerRef.current?.focus();
      }}
      className={[
        "w-full flex items-center gap-2 px-2.5 py-1.5 rounded-sm text-sm text-left",
        "outline-none transition-colors",
        "focus:bg-surface-muted hover:bg-surface-muted",
        tone === "danger" ? "text-danger" : "text-fg",
        active ? "bg-accent-soft text-accent-subtle" : "",
        disabled ? "opacity-50 cursor-not-allowed" : "",
        className,
      ].join(" ")}
      {...rest}
    >
      {leftIcon && <span className="shrink-0 [&_svg]:w-4 [&_svg]:h-4">{leftIcon}</span>}
      <span className="flex-1 min-w-0 truncate">{children}</span>
    </button>
  );
}

export function MenuSeparator() {
  return <div role="separator" className="my-1 h-px bg-line" />;
}
