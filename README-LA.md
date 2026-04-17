# fp-appimage-updater

[![Copr build status](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/status_image/last_build.png)](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/)
[![Documentation](https://img.shields.io/badge/docs-fau.fpt.icu-blue)](https://docs.fau.fpt.icu/)

# [🇺🇸](README.md) [🇧🇷](README-BR.md)

fp-appimage-updater est instrumentum CLI celere et unius binarii in Rust scriptum, designatum ad AppImages gerenda, renovanda et integranda omnino per YAML configurationes declarativas ab usore provisas. In spatio usoris stricte operans, destinatur ad usum cum dotfiles et perfecte cum Linux ambientibus immutabilibus/atomicis operatur.

## Proprietates
- **Data-Ductum:** Omnes appicationes et earum renovationis strategiae in YAML fasciculis definiuntur.
- **Renovationis Resolutores:** Novissimam versionem per Forge Releases (GitHub/GitLab/Gitea/Forgejo), Nexus Directos (ETag/Last-Modified HTTP Headers), aut Scripta Shell Consuetudinaria petere.
- **Renovationes Delta:** Backend internum `zsync-rs` utitur ad bytes tantum mutatos extrahendos cum reciperium appicationis id sinit.
- **Downloadationes Segmentatae:** Downloadationes directas magnas in HTTP ranges findere cum servitor eas sustinet. Defalta sinitur.
- **Operationes Parallelae:** `check` et `update` multas appicationes simul currunt ut batchas magnas celeriter servent, cum limitibus provisoris consciis ad eundem hostem non vexandum.
- **Intervalla Limitis Ratae:** Appicationes quae limites ratae tangunt praeteriuntur usque ad tempus rursus temptandi, nisi optaveris secus.
- **Auxilium Pro Tesserā GitHub:** Utere tessera accessus personalis per variabilem ambientis `GITHUB_TOKEN` aut `secrets.yml` ad limites ratae API GitHub (5,000 req/hora) praeteriendos.
- **GitHub Proxy Recursus:** Auxilium optionale pro GitHub metadata proxy limites ratae GitHub API praeterire potest sine ipsa downloadatione proxied, et multas bases proxy ordine temptare potest.
- **Integratio Scriptorii:** Manifesta `.desktop` et icones accurate directe ex AppImage extrahit utendo `--appimage-extract` et eas inconsutiliter in menum tuum appicationum `.local/share/applications` inserit.
- **Salutis Inspectiones Locales:** `doctor` configurationem localem, indices necessarios, et alia problemata constitutionis localis inspicit.
- **Configuratio Globalis & Localis:** Semitas parsimoniae, mores integrationis, symlinking, downloadationes segmentatas, intervalla limitis ratae, et praeposita GitHub proxy per appicationem aut globaliter superare.

## Facta Proiecti
- Hoc pro me feci quia taedebat me AppImages meas manu renovare et instrumentum volebam quod id pro me automatice faceret sine fasciculis meis configurationis delendis.
- Conlationes gratae sunt, sed memento proiectum simplex esse destinatum; quaelibet correctio erroris (bug fix) grata est, nullae proprietates extra scopum addentur.
- Consulto numquam repositorium pro reciperiis habebit, usores debent esse periti in suis reciperiis creandis.
- Est tantum binarium solum quod uti potes quomodocumque vis extra systemd servitium.
- Numquam GUI habebit, tantum instrumentum CLI est.

## Institutio

### 1. Fedora / OpenSUSE (COPR)
Si in distributione RPM-subnixa es, optima via ad `fp-appimage-updater` integrandum est per repositorium officiale COPR.

```bash
sudo dnf copr enable fptbb/fp-appimage-updater
sudo dnf install fp-appimage-updater
```

### 2. Scriptum Universale ad Celerem Institutionem

Pro omnibus aliis distributionibus Linux (etiam atomicis/immutabilibus), binarium solum inconsutiliter installare et temporaria systemd in scaena configurare potes utendo scripto nativo institutionis.

```bash
# Institutio defalta per totum usorem (~/.local/bin/ et ~/.config/systemd/user/)
curl -sL fau.fpt.icu/i | bash
```

Si inspectorem scaenae `systemd` automaticum nolis installari, potes addere `--no-systemd`:

```bash
curl -sL fau.fpt.icu/i | bash -s -- --no-systemd
```

Ad binarium et servitia stricte **per totum systema** disponenda (petens `/usr/bin/` et `/usr/lib/systemd/system/`), executionem explicite elevare debes. *(Nota: Si ambiens tuus actuosus stricte immutabilis est, scriptum hanc petitionem secure reiciet).*

```bash
curl -sL fau.fpt.icu/i | sudo bash -s -- --system
```

Ad renovatorem, eius binaria inconsutiliter deinstallanda, et eius temporaria DBus currentia clementer claudenda in quolibet scopo:

```bash
curl -sL fau.fpt.icu/i | bash -s -- --uninstall
```

### 3. Utendo Binariis Prae-structis

Novissima binaria compilata ex [paginis Releasearum](https://gitlab.com/fpsys/fp-appimage-updater/-/releases) officialibus deponere potes.
Pone binarium munde in folder binariorum praelatum (ex. gr. `~/.local/bin/`), curre `chmod +x`, et paratus es. Nativa ut executabile solum et isolatum fungitur, capax ad fluxus operis POSIX fovendos, etiam renovatio sui ipsius operatur.

### Compilatio ex Fonte

Si vis instrumentum ipse ex arbore fontis compilare, precor inspice directiones [CONTRIBUTING](https://www.google.com/search?q=CONTRIBUTING.md).

## Documentatio

Plena documentatio manet apud [docs.fau.fpt.icu](https://docs.fau.fpt.icu/). Teget fluxum constitutionis gradatim, formam reciperii, strategias renovationis, solutionem problematum, et singula inferioris gradus quae facilius in situ documentationis dedicato servari possunt quam in brevi README.

Si conaris intellegere quomodo mandatum se habeat vel cur appicatio praetereatur, illic primum incipe.

## Sectiones documentationis:

*preme ad expandendum*

<details>
<summary>1. Structura Indicis / Configuratio</summary>

### Instrumentum reciperia appicationum in folder tuo `~/.config/fp-appimage-updater/` expectat.

```
~/.config/fp-appimage-updater/
├── config.yml                # Mores globales (semitae parsimoniae, symlinks, integrationes)
└── apps/                     # Appicationes tuae
    ├── hayase/
    │   ├── app.yml           # Definitio pro Hayase
    │   └── resolver.sh       # Scriptum consuetudinarium si Strategia est 'script'
    └── whatpulse.yml         # Definitio utendo 'direct' Strategia via ETags
```

### Exemplum Configurationis Globalis (`config.yml`)

```yaml
storage_dir: ~/.local/bin/AppImages
symlink_dir: ~/.local/bin
naming_format: "{name}.AppImage"
manage_desktop_files: true
create_symlinks: false
segmented_downloads: true
respect_rate_limits: true
github_proxy: false
github_proxy_prefix:
  - "[https://gh-proxy.com/](https://gh-proxy.com/)"
  - "[https://corsproxy.io/](https://corsproxy.io/)?"
  - "[https://api.allorigins.win/raw?url=](https://api.allorigins.win/raw?url=)"
```

### Exemplum Reciperii App (`apps/whatpulse.yml`)

```yaml
name: whatpulse
strategy:
  strategy: direct
  url: "[https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage](https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage)"
  check_method: etag
segmented_downloads: true
```

### Zsync Renovationes Delta

`zsync` est semita downloadationis delta optionalis per appicationem, fulta backend interno `zsync-rs`. Tantum currit cum reciperium campum `zsync` includit et renovator invenire potest et AppImage iam installatum et manifestum `.zsync` congruens.

Formae reciperii fultae:

  - `zsync: true` significat renovatorem temptaturum `<resolved-download-url>.zsync`
  - `zsync: "https://example.org/file.AppImage.zsync"` significat renovatorem illo exacto URL manifesti usurum

Si renovatio delta quacumque de causa deficit, renovator monitionem imprimit et ad normalem semitam downloadationis HTTP recurrit.

Exemplum:

```yaml
name: my-app
strategy:
  strategy: forge
  repository: [https://github.com/example/my-app](https://github.com/example/my-app)
  asset_match: "my-app-*-x86_64.AppImage"
zsync: true
```

### Strategiae Renovationis

fp-appimage-updater tres diversas strategias ad renovationes solvendas et deponendas sustinet.

#### 1. forge

Usitata ad deponendum ex releaseis GitHub aut GitLab.

  - `repository`: URL ad repositorium GitHub aut GitLab.
  - `asset_match`: Textus wildcard ad congruendum nomen asset specifici in release (ex. gr., `"*-amd64.AppImage"`).
  - `asset_match_regex`: Matcher regex optionalis pro nomine fasciculi asset. Utere hoc cum glob nimis multa release assets congrueret. Regex contra plenum nomen asset confertur.
  - `github_proxy`: GitHub-tantum metadata proxy recursus optionalis per appicationem. Cum sinitur, `fp-appimage-updater` GitHub release API per bases proxy configuratas rursus temptat si petitio directa limitata est. Downloadatio finalis adhuc URL asset GitHub directo utitur.
  - `github_proxy_prefix`: URL basis proxy optionalis, array basium URL, aut textus `all` usitatus cum `github_proxy` sinitur. Defalta est `https://gh-proxy.com/`. App eas ordine temptat donec una operetur. Utere `all` ad omnem proxy compatibilem in instrumento structum temptandum.
  - `respect_rate_limits`: Superatio optionalis per appicationem quae renovatori dicit ut appicationes praetereat usque dum fenestra rursus temptandi expiret cum limes ratae tangitur. Defalta est `true`.

Pro repositoriis GitLab, resolutor forge utitur API permalink latest apud `https://gitlab.com/api/v4/projects/<project-path>/releases/permalink/latest`, legit `assets.links`, et mavult `direct_asset_url` cum praesto est.

**Exemplum:**

```yaml
strategy:
  strategy: forge
  repository: [https://github.com/hydralauncher/hydra](https://github.com/hydralauncher/hydra)
  asset_match: "hydralauncher-*.AppImage"
segmented_downloads: true
```

**Exemplum regex casus-limitis:**

```yaml
name: obsidian
strategy:
  strategy: forge
  repository: "[https://github.com/obsidianmd/obsidian-releases](https://github.com/obsidianmd/obsidian-releases)"
  asset_match_regex: "^Obsidian-[0-9.]+\\.AppImage$"
```

Hic regex congruit `Obsidian-1.12.7.AppImage` et vitat asset release `Obsidian-1.12.7-arm64.AppImage`.

#### 2. direct

Usitata cum appicatio URL downloadationis directum providet quod semper ad novissimam versionem punctat.

  - `url`: URL downloadationis staticum.
  - `check_method`: Quomodo detegatur si fasciculus remotus mutatus sit. Utere aut `etag` aut `last_modified`.
  - `segmented_downloads`: Superatio optionalis per appicationem pro downloadationibus HTTP range. Cum non definitur, flag globalis `segmented_downloads` adhibetur et defalta est `true`.

**Exemplum:**

```yaml
strategy:
  strategy: direct
  url: "[https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage](https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage)"
  check_method: etag
segmented_downloads: true
```

#### 3. script

Usitata pro scaenariis complexis ubi opus est scriptum bash consuetudinarium currere ad URL novissimum downloadationis et identificatorem versionis localis ad comparandum determinandum. Scriptum duas lineas emittere debet: URL downloadationis in prima linea, et textum versionis unicum in secunda linea.

  - `script_path`: Semita relativa ad scriptum bash locale.

**Exemplum:**

```yaml
strategy:
  strategy: script
  script_path: ./resolver.sh
segmented_downloads: true
```

Plura exempla in folder [examples/apps/](https://www.google.com/search?q=examples/apps/).

</details>
<br />
<details>
<summary>2. Renovationes in Scaena Systemd</summary>
<br />
Si appicationem scripto celeri institutionis installasti, temporarium systemd automatice configuratur ad inspectiones periodice in scaena currendas.

Quia hoc instrumentum stricte circa operationes spatii usoris designatum est, **noli uti `sudo`** cum eius servitiis systemd agis (nisi si per totum systema installasti, quo casu `sudo` et flag `--system` loco `--user` uti debes).

Inspice statum temporarii scaenae:

```bash
systemctl --user status fp-appimage-updater.timer
```

Vide novissimos logos executionis scaenae:

```bash
journalctl --user -u fp-appimage-updater.service -n 50
```

Sine aut incipe temporarium manu:

```bash
systemctl --user enable --now fp-appimage-updater.timer
```

</details>
<br />
<details>
<summary>3. Usus CLI</summary>

### Egressus JSON

Adde `--json` ad `init`, `validate`, `doctor`, `list`, `check`, `update`, aut `remove` cum egressum machinis-legibilem vis loco tabularum et linearum status.

### Configurationem Initiare

Crea fasciculos configurationis inchoantes pro config globali aut reciperio app specifici:

```bash
fp-appimage-updater init --global
```

Crea scenam reciperii app cum strategia renovationis electa:

```bash
fp-appimage-updater init --app whatpulse --strategy direct
```

Utere `--force` ad fasciculos existentes super-scribendos si opus est.

### Reciperia Validare

Valida omnes fasciculos reciperii appicationum configuratos:

```bash
fp-appimage-updater validate
```

Valida reciperium unicum per nomen app:

```bash
fp-appimage-updater validate whatpulse
```

Hoc mandatum inspicit ut fasciculi reciperii recte legantur et fasciculos invalidos refert ut eos corrigere possis ante renovationes currendas.

### Doctor

Curre inspectionem salutis celerem in constitutione locali:

```bash
fp-appimage-updater doctor
```

Hoc mandatum inspicit:

  - indicem config
  - indicem apps
  - fasciculum config globalem
  - indicem status (state)
  - utrum sera processus (process lock) desit, actuosa sit, aut exsoleta
  - utrum ulla reciperia feliciter lecta sint
  - utrum ulla reciperia legi non potuerint
  - utrum constitutio localis sana videatur pro operationibus renovationis

Inspice statum omnium reciperiorum tuorum configuratorum ad videndum si novae versiones remotae praesto sint:

```bash
fp-appimage-updater check
```

Inspice app unicum:

```bash
fp-appimage-updater check whatpulse
```

Egressus `check` nunc etiam indicia fulciminis refert cum praesto sunt, sicut fulcimen range downloadationis directae pro downloadationibus segmentatis et metadata resolutoris qua usus est ad versiones comparandas.

### Appicationes Renovare

Installa aut renova AppImage unicum:

```bash
fp-appimage-updater update whatpulse
```

Renova omnes configurationes simul:

```bash
fp-appimage-updater update
```

Renovationes felicies nunc tempus elapsum in secundis includunt ut videre possis quantum temporis quaeque app ad installandum aut renovandum cepit.
Cum renovator limitem ratae detegit, fenestram rursus temptandi meminit et illam app in proximo cursu praeterit nisi `respect_rate_limits` globaliter aut pro illa app debilitatur.
Appicationes forge GitHub optionaliter `github_proxy` cum textu aut array `github_proxy_prefix` consuetudinario uti possunt ad metadata rursus petenda per unum aut plures proxies sine ipsa URL downloadationis proxied.
Downloadationes cum parvo limite provisoris conscio disponuntur, ita renovator progreditur sine uno hoste onerando.

### Enumerare Apps Installatas

Recense integrationem praesentem et versiones locales appicationum tuarum definitarum:

```bash
fp-appimage-updater list
```

### Appicationes Removere

Remove binarium appicationis, symlink, icones extractas, et fasciculos desktop:

```bash
fp-appimage-updater remove whatpulse
```

Remove omnes appicationes installatas simul:

```bash
fp-appimage-updater remove -a
```

</details>
