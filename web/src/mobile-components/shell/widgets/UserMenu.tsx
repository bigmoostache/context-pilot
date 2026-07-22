import { Building2, LogOut, Settings, User as UserIcon, Users } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/mobile-components/ui/dropdown-menu"
import { useAccount } from "@/lib/providers/account"
import { useAuth } from "@/lib/providers/auth"
import { accentVar } from "@/lib/support/panelMeta"
import { canManageUsers } from "@/lib/support/roles"
import type { User } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Account avatar menu — mobile twin of `components/shell/widgets/UserMenu`.
 *
 * Same entry point (avatar → Profile / Settings / Manage Users / Sign out) and
 * same identity resolution (real signed-in user when auth on, else mock
 * account). The fork is touch sizing: the menu opens near-full-width
 * (`w-[min(...)]`) instead of a fixed 268px popover, every row is `py-2.5`
 * (≥44px tap target), and `active:` press feedback replaces the desktop
 * `hover:`. The dropdown primitive is the mobile action-sheet twin.
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
  const identity = authEnabled && authUser ? authUser : u
  const displayName = identity.name
  const displayEmail = identity.email
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label="Account menu"
        className={cn(
          "rounded-full transition-[filter,box-shadow] outline-none",
          "ring-1 ring-border focus-visible:ring-2 focus-visible:ring-(--signal)/60 active:brightness-105",
        )}
      >
        <AvatarMark user={u} initials={initialsOf(displayName)} className="size-8 text-[12px]" />
      </DropdownMenuTrigger>

      <DropdownMenuContent className="w-[min(17rem,calc(100vw-1.5rem))]" align="end" sideOffset={8}>
        {/* identity header — non-interactive (plain div: GroupLabel would
            require a Menu.Group ancestor and throws MenuGroupContext otherwise). */}
        <div className="flex items-center gap-2.5 p-2.5">
          <AvatarMark user={u} initials={initialsOf(displayName)} className="size-9 text-[13px]" />
          <div className="flex min-w-0 flex-col leading-tight">
            <span className="truncate text-[13px] font-semibold text-foreground/90">
              {displayName}
            </span>
            <span className="truncate text-[11.5px] font-normal text-muted-foreground/75">
              {displayEmail}
            </span>
            {(!authEnabled || !authUser) && <AccountPill user={u} />}
          </div>
        </div>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem onClick={onOpenProfile} className="gap-2.5 py-2.5 text-[13.5px]">
            <UserIcon className="size-4 text-(--interactive)" />
            Profile
          </DropdownMenuItem>
          <DropdownMenuItem onClick={onOpenSettings} className="gap-2.5 py-2.5 text-[13.5px]">
            <Settings className="size-4 text-muted-foreground" />
            Settings
          </DropdownMenuItem>
          {authEnabled && canManageUsers(authUser?.role) && onOpenUsers && (
            <DropdownMenuItem onClick={onOpenUsers} className="gap-2.5 py-2.5 text-[13.5px]">
              <Users className="size-4 text-muted-foreground" />
              Manage Users
            </DropdownMenuItem>
          )}
        </DropdownMenuGroup>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem
            variant="destructive"
            className="gap-2.5 py-2.5 text-[13.5px]"
            onClick={authEnabled ? () => void logout() : undefined}
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
      <span className="mt-1 inline-flex w-fit items-center gap-1 rounded-full bg-(--interactive)/12 px-1.5 py-px text-[9.5px] font-medium text-(--interactive)">
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
 * Shared by the trigger and the menu header so the identity reads consistently.
 * Exported (path parity with desktop) though mobile keeps its own callers.
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
        "flex shrink-0 items-center justify-center rounded-full font-semibold text-(--primary-foreground) select-none",
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
