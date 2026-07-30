type SettingsPageLayoutProps = {
  title: string;
  description?: string;
  footer?: React.ReactNode;
  children: React.ReactNode;
};

export function SettingsPageLayout({
  title,
  description,
  footer,
  children,
}: SettingsPageLayoutProps) {
  return (
    <div className="setup-step setup-step-center setup-step--stretch">
      <h2 className="setup-title">{title}</h2>
      {description && (
        <p className="setup-desc setup-desc--tight">{description}</p>
      )}
      <div className="settings-form">{children}</div>
      {footer}
    </div>
  );
}
