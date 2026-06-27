import { Building2, LogOut, Settings, User as UserIcon, Users } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useAccount } from "@/lib/support/account"
import { useAuth } from "@/lib/support/auth"
import { accentVar } from "@/lib/support/panelMeta"
import type { User } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Account avatar menu (T30) — the top-right entry point that replaced the old
 * global Settings gear. The avatar opens a floating menu offering **Profile**
 * and **Settings** (the latter still opens the very same {@link ConfigModal} the
 * gear used, so the change is purely a re-skin of the entry point and trivially
 * reversible). A header row previews who's signed in and whether the account is
 * company-managed.
 *
 * The menu is the project's portaled Base-UI dropdown, so it escapes the
 * TopBar's `.vibrancy` backdrop-filter containing block and gets keyboard a11y.
 */
export function UserMenu({
  onOpenSettings,
  onOpenProfile,
  onOpenUsers,
}: {
  onOpenSettings: () => void
  onOpenProfile: () => void
  /** Open the admin user-management dialog (Phase 10). */
  onOpenUsers?: () => void
}) {
  const { user: u } = useAccount()
  const { authEnabled, user: authUser, logout } = useAuth()
  // Prefer the real signed-in identity when auth is on; fall back to the mock
  // account otherwise (auth disabled has no real user).
  const displayName = authEnabled && authUser ? authUser.name : u.name
  const displayEmail = authEnabled && authUser ? authUser.email : u.email
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label="Account menu"
        className={cn(
          "rounded-full outline-none transition-[filter,box-shadow]",
          "ring-1 ring-border hover:brightness-105 focus-visible:ring-2 focus-visible:ring-[var(--signal)]/60",
        )}
      >
        <AvatarMark user={u} initials={initialsOf(displayName)} className="size-7 text-[11px]" />
      </DropdownMenuTrigger>

      <DropdownMenuContent className="w-[268px]" align="end" sideOffset={8}>
        {/* identity header — non-interactive (plain div: GroupLabel would
            require a Menu.Group ancestor and throws MenuGroupContext otherwise). */}
        <div className="flex items-center gap-2.5 px-2 py-2">
          <AvatarMark user={u} initials={initialsOf(displayName)} className="size-9 text-[13px]" />
          <div className="flex min-w-0 flex-col leading-tight">
            <span className="truncate text-[12.5px] font-semibold text-foreground/90">{displayName}</span>
            <span className="truncate text-[11px] font-normal text-muted-foreground/75">{displayEmail}</span>
            {(!authEnabled || !authUser) && <AccountPill user={u} />}
          </div>
        </div>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem onClick={onOpenProfile} className="gap-2.5 py-1.5 text-[12.5px]">
            <UserIcon className="size-4 text-[var(--interactive)]" />
            Profile
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onOpenSettings} className="gap-2.5 py-1.5 text-[12.5px]">
            <Settings className="size-4 text-muted-foreground" />
            Settings
          </DropdownMenuItem>
          {authEnabled && authUser?.role === "admin" && onOpenUsers && (
            <DropdownMenuItem onClick={onOpenUsers} className="gap-2.5 py-1.5 text-[12.5px]">
              <Users className="size-4 text-muted-foreground" />
              Manage Users
            </DropdownMenuItem>
          )}
        </DropdownMenuGroup>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem
            variant="destructive"
            className="gap-2.5 py-1.5 text-[12.5px]"
            onClick={authEnabled ? () => logout() : undefined}
          >
            <LogOut className="size-4" />
            Sign out
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** A small "Company account" / "Personal" pill summarising the management state. */
function AccountPill({ user }: { user: User }) {
  if (user.managedByCompany) {
    return (
      <span className="mt-1 inline-flex w-fit items-center gap-1 rounded-full bg-[var(--interactive)]/12 px-1.5 py-px text-[9.5px] font-medium text-[var(--interactive)]">
        <Building2 className="size-2.5" />
        {user.company ?? "Company account"}
      </span>
    )
  }
  return (
    <span className="mt-1 inline-flex w-fit items-center gap-1 rounded-full bg-muted px-1.5 py-px text-[9.5px] font-medium text-muted-foreground">
      Personal account
    </span>
  )
}

/**
 * The avatar fallback — a soft accent-gradient disc with the user's initials.
 * Shared by the trigger and the menu header (and the Profile modal) so the
 * identity reads consistently everywhere. No photo in the design; initials only.
 */
export function AvatarMark({
  user,
  initials,
  className,
}: {
  user: User
  /** Override the disc's initials (e.g. from the real signed-in user). */
  initials?: string
  className?: string
}) {
  const c = accentVar[user.accent]
  return (
    <span
      className={cn(
        "flex shrink-0 items-center justify-center rounded-full font-semibold text-[var(--primary-foreground)] select-none",
        className,
      )}
      style={{
        background: `linear-gradient(135deg, ${c}, color-mix(in oklab, ${c} 55%, #000))`,
      }}
    >
      {initials ?? user.initials}
    </span>
  )
}

/** Derive up-to-two-letter initials from a display name. */
function initialsOf(name: string): string {
  return (
    name
      .split(/\s+/)
      .filter(Boolean)
      .slice(0, 2)
      .map((w) => w[0]?.toUpperCase() ?? "")
      .join("") || "?"
  )
}
