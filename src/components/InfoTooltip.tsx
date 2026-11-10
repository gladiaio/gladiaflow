import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

const TOOLTIP_WIDTH = 260;
const GAP = 8;
const VIEWPORT_MARGIN = 8;

type Placement = "top" | "bottom";

type Coords = {
  left: number;
  top: number;
  placement: Placement;
};

/**
 * Hover/focus bubble around any trigger. Native `title` tooltips don't show
 * in the app's webview, so this one is drawn by hand. The bubble is rendered
 * in a portal with fixed positioning so it escapes scrollable containers
 * (which have `overflow-y: auto` and would otherwise clip it). Placement
 * flips between above/below the trigger depending on the room available, and
 * is clamped horizontally to the viewport.
 */
export function Tooltip({
  content,
  children,
  className,
  label,
  role,
  onClick,
  onKeyDown,
}: {
  content: ReactNode;
  children: ReactNode;
  className?: string;
  label?: string;
  role?: string;
  onClick?: (event: ReactMouseEvent<HTMLSpanElement>) => void;
  onKeyDown?: (event: ReactKeyboardEvent<HTMLSpanElement>) => void;
}) {
  const triggerRef = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);
  const [coords, setCoords] = useState<Coords | null>(null);
  const tooltipId = useId();

  const updatePosition = useCallback(() => {
    const trigger = triggerRef.current;
    if (!trigger) return;

    const rect = trigger.getBoundingClientRect();
    const centerX = rect.left + rect.width / 2;
    const left = Math.max(
      VIEWPORT_MARGIN,
      Math.min(
        centerX - TOOLTIP_WIDTH / 2,
        window.innerWidth - TOOLTIP_WIDTH - VIEWPORT_MARGIN,
      ),
    );

    // Prefer showing above the trigger, but flip below when there isn't
    // enough room near the top of the window.
    const spaceAbove = rect.top;
    const spaceBelow = window.innerHeight - rect.bottom;
    const placement: Placement =
      spaceAbove < 180 && spaceBelow > spaceAbove ? "bottom" : "top";

    const top = placement === "top" ? rect.top - GAP : rect.bottom + GAP;

    setCoords({ left, top, placement });
  }, []);

  const show = useCallback(() => {
    updatePosition();
    setOpen(true);
  }, [updatePosition]);

  const hide = useCallback(() => setOpen(false), []);

  useEffect(() => {
    if (!open) return;

    const onScroll = () => updatePosition();
    const onResize = () => updatePosition();
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };

    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [open, updatePosition]);

  return (
    <span
      ref={triggerRef}
      className={className}
      aria-label={label}
      tabIndex={0}
      role={role}
      aria-describedby={open ? tooltipId : undefined}
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
      onClick={onClick}
      onKeyDown={onKeyDown}
    >
      {children}
      {open &&
        coords &&
        createPortal(
          <span
            id={tooltipId}
            className={`info-tooltip info-tooltip--${coords.placement}`}
            role="tooltip"
            style={{
              left: `${coords.left}px`,
              top: `${coords.top}px`,
            }}
          >
            {content}
          </span>,
          document.body,
        )}
    </span>
  );
}

/** "i" icon showing `children` in a tooltip on hover/focus. */
export function InfoTooltip({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <Tooltip className="info-icon" label={label} role="img" content={children}>
      i
    </Tooltip>
  );
}
