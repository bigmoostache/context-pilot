// ── IT maintenance wizard (Milestone 5) ──────────────────────────────
//
// The provisioning flow served on :9090. Order is imposed (Obj 5.1–5.4):
//   login → forced password/email change → name/IP → CA trust → finalize.
// Once provisioned the console stays reachable and shows the post-provisioning
// view (Obj 5.5). The whole thing is driven off `GET /api/maint/status` + the
// admin's `must_change_password` flag.

import { useCallback, useEffect, useRef, useState } from "react"
import { getToken } from "@/lib/api/client"
import {
  fetchIdentity,
  fetchMaintStatus,
  fetchMaintMe,
  maintLogout,
  type Identity,
  type MaintStatus,
  type MaintUser,
} from "@/lib/api/maint"
import { Shell } from "./parts"
import { LoginStep } from "./LoginStep"
import { PasswordStep } from "./PasswordStep"
import { IdentityStep } from "./IdentityStep"
import { TrustStep } from "./TrustStep"
import { FinalizeStep } from "./FinalizeStep"
import { ProvisionedView } from "./ProvisionedView"

type Phase = "loading" | "login" | "password" | "wizard" | "provisioned"

export function MaintWizard({ initialStatus }: { initialStatus: MaintStatus }) {
  const [status, setStatus] = useState<MaintStatus>(initialStatus)
  const [me, setMe] = useState<MaintUser | null>(null)
  const [identity, setIdentity] = useState<Identity | null>(null)
  const [phase, setPhase] = useState<Phase>("loading")
  const [wizardStep, setWizardStep] = useState(0) // 0 identity, 1 trust, 2 finalize

  // Guards against setState after unmount (load() awaits several fetches).
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])

  // Recompute the whole flow from server truth (status + profile + identity).
  const load = useCallback(async () => {
    const token = getToken()
    const fresh = (await fetchMaintStatus().catch(() => null)) ?? initialStatus
    const user = token ? await fetchMaintMe().catch(() => null) : null
    const id = user ? (await fetchIdentity().catch(() => ({ identity: null }))).identity : null
    if (!alive.current) return
    setStatus(fresh)
    setMe(user)
    setIdentity(id)
    if (!user) setPhase("login")
    else if (user.must_change_password) setPhase("password")
    else if (fresh.provisioned) setPhase("provisioned")
    else {
      setWizardStep(fresh.identity_set ? 1 : 0)
      setPhase("wizard")
    }
  }, [initialStatus])

  useEffect(() => {
    void load()
  }, [load])

  const logout = useCallback(async () => {
    await maintLogout()
    setMe(null)
    setPhase("login")
  }, [])

  if (phase === "loading") {
    return <Shell title="Loading…">{null}</Shell>
  }

  if (phase === "login") {
    return (
      <Shell title="Sign in" subtitle="Use the admin credentials from the delivery sheet.">
        <LoginStep onSuccess={() => void load()} />
      </Shell>
    )
  }

  if (phase === "password" && me) {
    return (
      <Shell title="Secure the admin account" subtitle="Change the delivered password before continuing.">
        <PasswordStep user={me} onDone={() => void load()} />
      </Shell>
    )
  }

  if (phase === "provisioned") {
    return (
      <Shell title="Appliance is live">
        <ProvisionedView
          onReconfigure={() => {
            setWizardStep(0)
            setPhase("wizard")
          }}
          onLogout={() => void logout()}
        />
      </Shell>
    )
  }

  // phase === "wizard"
  const step = { current: wizardStep, total: 3 }
  if (wizardStep === 0) {
    return (
      <Shell title="Name this appliance" step={step}>
        <IdentityStep
          initial={identity}
          onDone={() => {
            void fetchIdentity().then((r) => setIdentity(r.identity))
            setStatus((s) => ({ ...s, identity_set: true }))
            setWizardStep(1)
          }}
        />
      </Shell>
    )
  }
  if (wizardStep === 1) {
    return (
      <Shell title="Trust the certificate" step={step}>
        <TrustStep onDone={() => setWizardStep(2)} />
      </Shell>
    )
  }
  return (
    <Shell title="Finish setup" step={step}>
      <FinalizeStep
        status={status}
        cockpitName={identity?.name || identity?.ip || ""}
        passwordChanged={!!me && !me.must_change_password}
      />
    </Shell>
  )
}
