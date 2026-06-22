# Brief commercial — Context Pilot as-a-Service

**Objet** : déploiement d'une solution d'assistance IA professionnelle sous forme d'appliance sécurisée pré-configurée, louée en tant que service managé.

**Client** : Cabinet de conseil Attali

**Date** : 22 juin 2026

---

## 1. Description du service

### 1.1 Produit

**Context Pilot** est une plateforme d'assistance IA multi-agents destinée aux professionnels. Elle combine :

- Un **assistant IA conversationnel** avec accès à des outils de recherche web, d'analyse documentaire et de gestion de connaissances
- Un **orchestrateur multi-agents** permettant la gestion simultanée de plusieurs sessions de travail indépendantes
- Un **moteur de recherche sémantique** indexant automatiquement les documents et projets du client
- Des capacités de **recherche web avancée** (Brave Search), de **scraping** (Firecrawl), d'**OCR** (Datalab) et d'**embeddings sémantiques** (Voyage AI)
- Une **interface web** moderne accessible depuis tout navigateur sur le réseau local

### 1.2 Forme de déploiement

Le service est déployé sur un **boîtier dédié Photonicat 2** (processeur ARM Rockchip RK3576, 16 Go de RAM, 128 Go de stockage interne + SSD NVMe 256 Go). Ce boîtier est :

- **Pré-configuré** : système Debian durci, logiciel installé et testé, prêt à l'emploi
- **Sécurisé** : les données du client restent physiquement dans leurs locaux (on-premise)
- **Autonome** : batterie intégrée (~24h), double Ethernet Gigabit, Wi-Fi 6, connectique USB 3.0 et HDMI
- **Propriété du prestataire** : le matériel est loué, non vendu

### 1.3 Capacité

Le forfait couvre jusqu'à **5 postes simultanés** (agents IA indépendants).

---

## 2. Grille tarifaire

### 2.1 Frais de mise en service (facturation unique)

| Désignation | Montant HT |
|-------------|------------|
| Fourniture et configuration du boîtier Photonicat 2 (16 Go / 128 Go + SSD NVMe 256 Go) | |
| Installation du système d'exploitation (Debian), durcissement sécuritaire | |
| Installation et configuration de Context Pilot (orchestrateur, moteur de recherche, services auxiliaires) | |
| Tests de validation et recette technique | |
| **Total frais de mise en service** | **750,00 €** |

### 2.2 Abonnement mensuel (forfait équipe — jusqu'à 5 postes)

| Désignation | Montant HT / mois |
|-------------|-------------------|
| Location du matériel (boîtier Photonicat 2 + SSD) | |
| Licence de la plateforme Context Pilot (configuration enterprise, orchestrateur multi-agents, interface web) | |
| Services auxiliaires inclus : recherche web (Brave Search), scraping (Firecrawl), embeddings sémantiques (Voyage AI), OCR documentaire (Datalab) | |
| Maintien en condition opérationnelle : mises à jour logicielles, monitoring, correctifs de sécurité | |
| Support par email | |
| **Total abonnement mensuel** | **250,00 € / mois** |

### 2.3 Accompagnement (obligatoire les 3 premiers mois)

| Désignation | Tarif | Volume | Total HT |
|-------------|-------|--------|----------|
| Journée d'accompagnement sur site ou à distance (formation, optimisation des workflows, intégration dans les pratiques du cabinet) | 500,00 € / jour | 3 jours minimum, répartis sur les mois 1 à 3 | **1 500,00 €** |

> Après la période initiale de 3 mois, l'accompagnement reste disponible à la demande au même tarif journalier.

---

## 3. Récapitulatif financier

### 3.1 Année 1

| Poste | Montant HT |
|-------|------------|
| Frais de mise en service | 750,00 € |
| Abonnement mensuel × 12 mois | 3 000,00 € |
| Accompagnement obligatoire (3 jours) | 1 500,00 € |
| **Total année 1** | **5 250,00 €** |

### 3.2 À partir de l'année 2

| Poste | Montant HT |
|-------|------------|
| Abonnement mensuel × 12 mois | 3 000,00 € |
| **Total annuel** | **3 000,00 €** |

> Accompagnement optionnel en sus : 500,00 € HT / jour.

---

## 4. Transparence des coûts

### 4.1 Coûts réels supportés par le prestataire (par mois)

Les services cloud auxiliaires sont mutualisés entre plusieurs clients. Les coûts ci-dessous reflètent la **quote-part imputable à ce contrat** (¼ des abonnements).

| Poste | Coût mensuel réel |
|-------|-------------------|
| Brave Search (recherche web) — ¼ abonnement | ~11 € |
| Firecrawl (scraping) — ¼ abonnement | ~4 € |
| Voyage AI (embeddings sémantiques) — ¼ abonnement | ~0,50 € |
| Datalab OCR (reconnaissance documentaire) — ¼ abonnement | ~1,25 € |
| **Sous-total services cloud** | **~17 €** |
| Amortissement matériel (Photonicat 2 + SSD, sur 18 mois) | ~12 € |
| Maintien en condition opérationnelle (~2h/mois) | ~100 € |
| **Total coûts réels mensuels** | **~129 €** |

### 4.2 Marge du prestataire

| | Montant |
|---|---------|
| Facturation mensuelle | 250 € |
| Coûts réels mensuels | ~129 € |
| **Marge brute** | **~121 € / mois (48 %)** |

> La marge couvre : le temps de développement du logiciel (3 mois, 65 000+ lignes de code, architecture 22 modules), le risque opérationnel (remplacement matériel, incidents), l'expertise technique et l'évolution continue de la plateforme.

---

## 5. Périmètre

### 5.1 Inclus dans le service

- Matériel (location) : boîtier Photonicat 2 pré-configuré
- Logiciel : Context Pilot (orchestrateur, interface web, moteur de recherche)
- Services cloud auxiliaires :
  - **Brave Search** — recherche web
  - **Firecrawl** — extraction de contenu de pages web
  - **Voyage AI** — embeddings pour la recherche sémantique
  - **Datalab** — reconnaissance optique de caractères (OCR)
  - **Meilisearch** — moteur de recherche local (hébergé sur le boîtier)
- Maintien en condition opérationnelle (MCO)
- Support par email
- Mises à jour logicielles

### 5.2 Exclu du service (à la charge du client)

| Élément | Détail | Estimation |
|---------|--------|------------|
| **Fournisseur de modèle IA (LLM)** | Clé API souscrite directement par le client auprès du fournisseur de son choix | 50 à 300 € / mois selon le modèle et l'intensité d'utilisation |

> Le fournisseur recommandé est **Anthropic** (modèle Claude). Anthropic offre les meilleures garanties en termes de fiabilité, de qualité de raisonnement et de protection des données. Voir la politique de confidentialité détaillée sur notre Trust Center :
>
> 🔗 **[Trust Center — Sous-traitants & conformité](https://bigmoostache.github.io/context-pilot/trust-center/subprocessors.html)**
>
> Points clés Anthropic :
> - Certifié **SOC 2 Type II** et **ISO 27001**
> - Les données API ne sont **jamais utilisées pour l'entraînement** des modèles
> - Rétention zéro des prompts après traitement (sauf obligation légale)
> - **DPA** (Data Processing Agreement) disponible sur demande
> - Siège : États-Unis (San Francisco), juridiction californienne

---

## 6. Conditions contractuelles

### 6.1 Durée et renouvellement

- **Engagement initial** : 12 mois à compter de la date de mise en service
- **Renouvellement** : tacite reconduction pour une nouvelle période de 12 mois à la date anniversaire
- **Résiliation** : le client peut notifier sa décision de ne pas renouveler à tout moment avant la date anniversaire, sans préavis. La résiliation prend effet à l'échéance de la période en cours.

### 6.2 Facturation

- **Frais de mise en service** : facturés à la commande
- **Accompagnement obligatoire** : facturé à la réalisation de chaque journée
- **Abonnement mensuel** : facturé mensuellement, à terme à échoir

### 6.3 Propriété du matériel

Le boîtier Photonicat 2 reste la propriété exclusive du prestataire pendant toute la durée du contrat. En cas de résiliation, le matériel est restitué au prestataire dans un délai de 15 jours suivant l'échéance.

### 6.4 Protection des données

- Les données du client sont stockées localement sur le boîtier, dans les locaux du client
- Aucune donnée n'est transmise à des tiers, à l'exception des requêtes envoyées au fournisseur de modèle IA choisi par le client et aux services auxiliaires listés en §5.1
- Les services auxiliaires sont souscrits et gérés par le prestataire ; leur politique de traitement des données est documentée dans le Trust Center (lien ci-dessus)
- Le prestataire s'engage à ne pas accéder aux données du client sauf intervention de maintenance convenue

### 6.5 Limitation de capacité

Le forfait est dimensionné pour un usage simultané de **5 postes maximum**. Au-delà, un second boîtier et un forfait additionnel seraient nécessaires (conditions à définir).

---

## 7. Notes à l'attention du directeur financier

Ce document contient l'ensemble des éléments commerciaux nécessaires à l'établissement du devis. Les montants sont exprimés **hors taxes**.

Récapitulatif des lignes de facturation :

| # | Désignation | Type | Montant HT |
|---|-------------|------|------------|
| 1 | Frais de mise en service | Unique | 750,00 € |
| 2 | Abonnement mensuel — forfait équipe (5 postes) | Récurrent (mensuel) | 250,00 € / mois |
| 3 | Accompagnement — journée (obligatoire M1-M3, optionnel ensuite) | À la journée | 500,00 € / jour |
