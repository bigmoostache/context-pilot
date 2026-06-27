# Runbook day-0 — box d'usine → jointe au tailnet, prête pour Ansible

> Procédure **manuelle** (Phase 1), à dérouler une fois par box neuve, à la main,
> via l'AP WiFi de la box, **avant tout Ansible**. Une fois éprouvée, elle devient
> `bootstrap.sh` (Phase 2), puis est gravée dans l'image via `/etc/uci-defaults`
> (industrialisation). Réfs : [`../PROVISIONING.md`](../PROVISIONING.md) (procédure complète +
> décisions & caveats), service procd livré ici : [`tailscale.init`](./tailscale.init).
>
> Placeholders : `<client>` (ex. `acme`), `<unit>` (ex. `acme-01`),
> `<tailnet>` (ex. `tail7390da`), `<AUTHKEY>` (`tskey-auth-…`, depuis le Vault).

---

## 0. Prérequis control-plane (fait UNE fois — cf. stratégie §1–§5)

- Le tailnet existe ; la **policy file est SAUVÉE** (pas seulement prévisualisée) avec :
  - `groups.group:ops` = tes identités ops,
  - `tagOwners."tag:cp-<client>"` = `["group:ops"]`,
  - `acls` (group:ops → tag:cp-<client> sur `:22/:9090/:7878`) et `ssh` (group:ops → tag:cp-<client>, root).
- Une **auth-key taguée, reusable, non-ephemeral, pre-approved** générée → `<AUTHKEY>`.
- Le **control node** (ton poste) est lui-même sur le tailnet.

> Si l'enrôlement échoue plus tard avec `requested tags … invalid or not permitted`,
> c'est que la policy n'a pas été **Save** — corrige et Save, puis recommence (cf. Troubleshooting).

---

## 1. Joindre la box d'usine

Après un factory reset, la box n'est que sur son AP WiFi. Connecte-toi à ce WiFi, puis :
```sh
ssh root@172.16.0.1           # mot de passe root d'usine
uname -m                      # attendu : aarch64
cat /etc/openwrt_release       # attendu : photonicatWrt … rockchip/armv8
ping -c1 1.1.1.1 && nslookup pkgs.tailscale.com   # internet + DNS sortants (Tailscale = 443 sortant)
```

## 2. (Optionnel) poser une clé SSH ops — break-glass uniquement

Tailscale SSH rend la clé inutile en exploitation normale. N'en pose une que comme **issue de secours
LAN** (Tailscale/DERP down + intervention sur site). Depuis le control node :
```sh
ssh-copy-id -i ~/.ssh/id_ed25519.pub root@172.16.0.1
```

## 3. Installer Tailscale

> **⚠️ SÉCURITÉ — version.** Le feed opkg de photonicatWrt 25.02.0 fige Tailscale à **1.76.1**
> (CVE connue ; la console signale « Security update available → 1.98.4 »). `opkg upgrade` ne tire que
> ce que le feed contient → **ne corrige pas**. Pour un démon exposé, ne pas livrer une version périmée.

### Méthode A — binaire statique officiel (recommandée : à jour & patchable) ⚠️ *à valider sur la box*
Les binaires Linux de Tailscale sont du Go pur statique (pas de dépendance libc) → tournent sous musl.
```sh
opkg list-installed | grep -q ca-bundle || { opkg update && opkg install ca-bundle kmod-tun; }
cd /tmp && V=1.98.4                            # ou la dernière stable ; aarch64 = arm64
wget "https://pkgs.tailscale.com/stable/tailscale_${V}_arm64.tgz"
tar xzf "tailscale_${V}_arm64.tgz"
install -m 0755 "tailscale_${V}_arm64/tailscale"  /usr/sbin/tailscale
install -m 0755 "tailscale_${V}_arm64/tailscaled" /usr/sbin/tailscaled
tailscale version                              # confirmer ${V}
```

### Méthode B — package opkg (rapide, mais 1.76.1 = CVE) — *fallback validé*
```sh
opkg update && opkg install tailscale          # deps : libc, ca-bundle, kmod-tun
```
Le package est *bare* : il ne livre que `/usr/sbin/tailscale{,d}`, **pas de `/etc/init.d`** → on fournit
le nôtre (étape 4). Maintenir Tailscale à jour est lui-même une instance de la stratégie d'update
(cf. PROVISIONING.md, MàJ app) : version pinnée + chemin de refresh contrôlé.

## 4. Installer le service procd

Le package ne livre pas d'init script. Copie le nôtre depuis le control node :
```sh
scp deploy/photonicat/tailscale.init root@172.16.0.1:/etc/init.d/tailscale
```
Puis sur la box :
```sh
chmod +x /etc/init.d/tailscale
/etc/init.d/tailscale enable
/etc/init.d/tailscale start
sleep 3
ps w | grep '[t]ailscaled'     # démon supervisé par procd
tailscale status               # attendu : "Logged out."
```
L'état persiste sous `/mnt/data/context-pilot/tailscale/` → la box rejoint le tailnet aux reboots.
(Sur une box reset, `/mnt/data` n'est pas encore une partition séparée — retombe sur l'overlay f2fs
persistant ; sans impact pour Tailscale.)

## 5. Enrôler la box (one-shot)
```sh
tailscale up \
  --authkey=<AUTHKEY> \
  --advertise-tags=tag:cp-<client> \           # nœud devient propriété du tag (expiry off, ACL/SSH s'appliquent)
  --hostname=<unit> \                           # → MagicDNS <unit>.<tailnet>.ts.net
  --ssh \                                        # active Tailscale SSH sur la box
  --accept-routes=false                          # ne PAS tirer les routes du tailnet
# (on n'utilise PAS --advertise-routes → le LAN client n'est jamais exposé)
```
Vérifier sur la box :
```sh
tailscale ip -4                                # l'IP 100.x du tailnet
tailscale status --json | grep -A2 '"Tags"'    # attendu : tag:cp-<client>
```

## 6. Vérifier l'accès distant depuis le control node
```sh
tailscale ping <unit>                          # joignabilité overlay
tailscale ssh root@<unit> 'echo OK; uname -n'  # SANS clé — Tailscale SSH via identité tailnet
```
Dans la console Tailscale, confirmer que `<unit>` affiche **« Key expiry disabled »** (invariant des
nœuds tagués ; le JSON de status peut montrer un timestamp trompeur).

## 7. Passer la main à Ansible

La box est joignable en `<unit>.<tailnet>.ts.net`. Pointe l'inventaire sur ce nom MagicDNS (cf.
`deploy/ansible/inventory.example.ini`) — plus de contrainte « même LAN » :
```sh
ansible -i deploy/ansible/inventory.ini <client> -m ping
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml --limit <client> --ask-vault-pass
```

---

## Troubleshooting

| Symptôme | Cause / correctif |
|---|---|
| `requested tags [tag:cp-<client>] are invalid or not permitted` | Policy file non **Save** avec `tagOwners` + `group:ops`, ou ton identité absente de `group:ops`. Corrige (login.tailscale.com/admin/acls/file), Save, recommence. |
| `changing settings via 'tailscale up' requires mentioning all non-default flags` | Une préf est collée. Relance avec `--reset` + le jeu complet de flags. |
| Le nœud apparaît sous une identité user (pas le tag) | Enrôlé sans tag / mauvais tag. Refais l'étape 5 avec `--reset --advertise-tags=tag:cp-<client>`. |
| `tailscale ssh -o …` erreur | `tailscale ssh` ne prend pas de flags `-o` ; passe `[user@]host [commande]` seulement. |
| Le démon ne démarre pas | `logread -e tailscaled` ; vérifie `/dev/net/tun` présent et `kmod-tun` installé. |

## ⚠️ Mettre à jour Tailscale plus tard (hors day-0)

Le binaire statique (Méthode A) est **validé en 1.98.4** ; le feed opkg fige 1.76.1 (CVE), à éviter.
Pour upgrader une box **déjà en prod, joignable seulement par le tailnet**, attention :
- `ssh root@<unit>.<tailnet>` passe par **Tailscale SSH** = session **fille de `tailscaled`** →
  un `/etc/init.d/tailscale restart` **tue ta propre session**. Et **busybox n'a pas `setsid`**.
- Faire le swap **détaché de la session** : `start-stop-daemon -b -x /path/script` (double-fork,
  survit), ou se connecter par une voie indépendante (LAN break-glass / dropbear).
- À évaluer comme voie propre : `tailscale update` (le binaire statique le supporte et gère le
  self-replace + restart correctement). Voir la stratégie d'update dans `../PROVISIONING.md`.

Swap manuel (depuis une session **indépendante de tailscaled**, ex. LAN) :
```sh
V=1.98.4; cd /tmp
wget "https://pkgs.tailscale.com/stable/tailscale_${V}_arm64.tgz" && tar xzf "tailscale_${V}_arm64.tgz"
/etc/init.d/tailscale stop
cp tailscale_${V}_arm64/tailscale  /usr/sbin/tailscale
cp tailscale_${V}_arm64/tailscaled /usr/sbin/tailscaled
/etc/init.d/tailscale start                    # tag + état persistent ; le nœud rejoint seul
tailscale status                               # connecté (PAS "stopped")
```

## Hygiène post-provisioning
- **Révoquer l'auth-key** de provisioning si elle a pu fuiter (console → Settings → Keys).
- Pour de vraies unités : poser le `<unit>` et le `tag:cp-<client>` définitifs.
- Confirmer **key expiry disabled** sur le device tagué.
