const LOGO_SRC = "/Images/Logo_Gladia_flow.svg";

export function GladiaFlowLogo({ className }: { className?: string }) {
  return (
    <img
      src={LOGO_SRC}
      alt="GladiaFlow"
      className={className ?? "logo-wordmark"}
      draggable={false}
    />
  );
}
