# L'écran LCD du Photonicat 2 (Armbian / Debian 13)

Guide de mise en route de l'écran LCD frontal du Photonicat 2 sous Armbian /
Debian 13 (systemd). Objectif : comprendre comment il marche et le faire
fonctionner du premier coup.

> Vérifié sur Armbian 26.8 (trixie), kernel 6.18 rockchip64, aarch64, avec le
> driver [`photonicat/photonicat2_mini_display`](https://github.com/photonicat/photonicat2_mini_display).

---

## 1. Comment ça marche

Pour que l'écran affiche quelque chose, **deux mécanismes indépendants** doivent
être actifs en même temps :

1. **Le rétroéclairage** — la lumière derrière la dalle.
2. **La boucle de rendu** — les pixels envoyés à la dalle.

Si l'un des deux manque : rétroéclairage éteint → dalle noire ; boucle arrêtée →
rétroéclairage allumé mais image figée. Il faut **les deux**.

```
        ┌─────────────────────────┐
        │    Dalle LCD GC9307     │   172×320, tournée 180°
        └──────┬──────────┬───────┘
        données│ SPI      │ GPIO contrôle          lumière
         /dev/spidev1.0   DC = gpio121          ┌───────────────┐
                          RST = gpio122         │ PWM backlight │
                          CS  = gpio13          │  pwmchip0     │
                                                └───────┬───────┘
                                        /sys/class/backlight/backlight
```

- **La dalle** est un contrôleur **GC9307** branché en **SPI** (`/dev/spidev1.0`).
  Les pixels y sont poussés octet par octet, accompagnés de deux GPIO : **DC**
  (commande vs donnée) et **RST** (reset).
- **Ce n'est pas un framebuffer Linux** : pas de `/dev/fb0` pour cette dalle.
  Seul un programme qui parle directement au SPI peut l'alimenter — c'est le rôle
  du driver ci-dessous.
- **Le rétroéclairage est une LED PWM séparée**, exposée par le kernel comme un
  `pwm-backlight` standard dans `/sys/class/backlight/backlight`.

**Le driver** `pcat2_mini_display` (un programme Go lancé par systemd) fait
tourner une boucle : il calcule l'image de la page courante en mémoire, l'envoie
sur le SPI, gère les transitions de page, la luminosité et la mise en veille. Il
expose aussi une petite API HTTP de contrôle (§6).

### Cartographie matérielle

| Fonction         | Ressource Linux |
|------------------|-----------------|
| Données SPI      | `/dev/spidev1.0` |
| DC (data/cmd)    | `gpiochip3` ligne **121** |
| RST (reset)      | `gpiochip3` ligne **122** |
| CS (chip select) | GPIO **13** |
| Rétroéclairage   | `/sys/class/backlight/backlight` (`pwm-backlight`, `pwmchip0`) |

---

## 2. Prérequis

- **Le bus SPI et les GPIO doivent être exposés par le device-tree.** L'image
  Armbian officielle du Photonicat les active déjà — vérifiez avec :
  ```sh
  ls /dev/spidev*      # doit afficher /dev/spidev1.0
  ```
- **Accès réseau pendant l'installation** : le driver se compile sur la box, qui
  doit joindre GitHub et le proxy Go (le Photonicat est un routeur, il a du WAN).

---

## 3. Installation

### Voie recommandée — le playbook Ansible de ce repo

L'écran est intégré au déploiement appliance (`deploy/ansible/site.yml`, tâche
`deploy/ansible/tasks/display.yml`). Un déploiement normal l'installe et le
configure automatiquement :

```sh
ansible-playbook -i deploy/ansible/inventory.ini deploy/ansible/site.yml
```

La tâche : installe la toolchain (`golang build-essential git`), clone le driver,
le **compile nativement sur la box** (arm64, CGO), l'installe, **règle la veille
pour que l'écran reste allumé** (§5), puis active et démarre le service.

Options utiles (`-e ...`) :

```sh
-e install_display=false                 # ne pas installer l'écran sur cette box
-e display_ref=<sha|branche>             # épingler une version du driver (défaut: main)
-e display_battery_dimmer_seconds=86400  # délai avant mise en veille (défaut: 24h)
```

### Voie manuelle (sur la box, arm64)

```sh
sudo apt update && sudo apt install -y golang build-essential git

git clone https://github.com/photonicat/photonicat2_mini_display.git
cd photonicat2_mini_display          # ⚠️ la branche par défaut est "main", pas "master"
CGO_ENABLED=1 go build -o pcat2_mini_display_debian .

sudo install -m0755 pcat2_mini_display_debian /usr/local/bin/pcat2_mini_display
sudo cp config.json /etc/pcat2_mini_display-config.json
sudo mkdir -p /usr/local/share/pcat2_mini_display
sudo cp -ar assets /usr/local/share/pcat2_mini_display/
sudo cp service-files/pcat2_mini_display.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now pcat2_mini_display
```

> Les chemins config/assets sont codés en dur en absolu dans le binaire et
> l'unité ne fixe pas de `WorkingDirectory` — ne les déplacez pas.

### Fichiers installés

| Élément | Chemin |
|---------|--------|
| Binaire | `/usr/local/bin/pcat2_mini_display` |
| Config  | `/etc/pcat2_mini_display-config.json` |
| Assets (polices, svg, html) | `/usr/local/share/pcat2_mini_display/assets/` |
| Unité systemd | `/etc/systemd/system/pcat2_mini_display.service` |

### Piloter le service

```sh
systemctl status  pcat2_mini_display     # état
systemctl restart pcat2_mini_display     # relancer (réinitialise la dalle, recharge la config)
journalctl -u pcat2_mini_display -f      # logs en direct
```

---

## 4. Le réglage à connaître : garder l'écran allumé

Par défaut, le driver **éteint le rétroéclairage après 60 secondes** d'inactivité.

Il applique en fait deux délais selon l'alimentation — `screen_dimmer_time_on_
battery_seconds` (60 s) sur batterie, `screen_dimmer_time_on_dc_seconds` (24 h)
sur secteur — et choisit lequel via le démon **`pcat-manager`** (§7). **Sans
`pcat-manager`, le driver se croit toujours sur batterie** et coupe donc l'écran
au bout d'une minute.

La solution simple (appliquée automatiquement par le playbook Ansible) : allonger
le délai batterie.

```sh
sudo sed -i -E 's/("screen_dimmer_time_on_battery_seconds": )[0-9]+/\186400/' \
  /etc/pcat2_mini_display-config.json
sudo systemctl restart pcat2_mini_display
```

---

## 5. Configuration — `/etc/pcat2_mini_display-config.json`

| Clé | Rôle | Défaut |
|-----|------|--------|
| `screen_dimmer_time_on_battery_seconds` | Délai de veille **sur batterie** | `60` |
| `screen_dimmer_time_on_dc_seconds`      | Délai de veille **sur secteur** | `86400` |
| `screen_max_brightness` | Luminosité max (0–100) | `100` |
| `screen_min_brightness` | Luminosité mini | `0` |
| `show_sms` / `sms_limit_for_screen` | Affichage des SMS | `true` / `5` |
| `ping_site0`, `ping_site1` | Cibles de ping affichées | `1.1.1.1`, `photonicat.com` |

Après édition : `systemctl restart pcat2_mini_display`.

> À propos de la luminosité : le driver écrit `screen_max_brightness` (borné à
> 100) directement dans un sysfs qui va jusqu'à 255. Son « 100 % » vaut donc
> ~39 % du rétroéclairage matériel — lisible, mais plus sombre qu'un réglage
> manuel à 255 (§6).

---

## 6. Piloter l'écran à la main

### Rétroéclairage (sysfs)

```sh
# Allumer / éteindre  (0 = allumé, 4 = éteint)
echo 0 | sudo tee /sys/class/backlight/backlight/bl_power

# Luminosité (0..255)
echo 255 | sudo tee /sys/class/backlight/backlight/brightness
cat /sys/class/backlight/backlight/max_brightness      # 255
```

> Ces écritures sysfs sont volatiles (perdues au reboot et reprises par le
> driver). Pour un réglage durable, passez par la config (§5).

### API HTTP de contrôle

Le driver écoute en local sur **`http://127.0.0.1:8081`**, routes sous `/api/v1`.

| Méthode & route | Effet |
|-----------------|-------|
| `GET  /api/v1/go_get_status.json` | État du service |
| `GET  /api/v1/go_frame.png` | Capture PNG 172×320 de l'écran (buffer interne) |
| `GET  /api/v1/go_changePage` | Page suivante |
| `GET  /api/v1/go_make_it_run` | (Re)lance la boucle de rendu |
| `GET  /api/v1/go_get_config.json` | Config effective |
| `POST /api/v1/go_set_screen_dimmer_time` | Change les délais de veille |
| `GET  /api/v1/go_get_max_backlight` · `POST /api/v1/go_set_max_backlight` | Lit / règle la luminosité max |
| `POST /api/v1/go_set_show_sms` · `POST /api/v1/go_set_ping_sites` | Réglages d'affichage |
| `GET  /api/v1/go_display_text.json?text=…` | Dessine du texte ponctuel — **met la boucle de rendu en pause** (voir note) |

```sh
# Voir ce qui est affiché, sans regarder la box
curl -s http://127.0.0.1:8081/api/v1/go_frame.png -o screen.png

# Depuis un poste distant (l'API n'écoute qu'en loopback) → tunnel SSH
ssh -L 8081:127.0.0.1:8081 root@<box>
curl -s http://127.0.0.1:8081/api/v1/go_frame.png -o screen.png

# Page suivante
curl -s http://127.0.0.1:8081/api/v1/go_changePage
```

> **`go_display_text.json` met la boucle de rendu en pause** (il dessine une fois
> puis l'affichage ne se met plus à jour). Réservez-le à un affichage volontaire ;
> pour reprendre l'affichage normal, appelez `go_make_it_run` ou redémarrez le
> service.

---

## 7. Composants compagnons (widgets complets)

L'écran fonctionne sans eux, mais certains widgets restent vides. Pour un
affichage complet :

- **`pcat-manager`** — démon d'alimentation / modem de photonicat
  ([`rockchip_rk3568_pcat_manager`](https://github.com/photonicat/rockchip_rk3568_pcat_manager)).
  Fournit l'état secteur/batterie (donc la bonne détection de veille, §4), le
  modem et le bouton de réveil.
- **`vnstat`** — statistiques de bande passante (`apt install vnstat`), pour les
  widgets « Daily / Monthly Data Usage ».

Ces deux composants ne sont pas installés par le playbook aujourd'hui.
