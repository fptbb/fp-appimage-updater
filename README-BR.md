# fp-appimage-updater

[![Copr build status](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/status_image/last_build.png)](https://copr.fedorainfracloud.org/coprs/fptbb/fp-appimage-updater/package/fp-appimage-updater/)
[![Documentation](https://img.shields.io/badge/docs-fau.fpt.icu-blue)](https://docs.fau.fpt.icu/)

# [🇺🇸](README.md)

fp-appimage-updater é uma ferramenta CLI rápida, de binário único, escrita em Rust, projetada pra gerenciar, atualizar e integrar AppImages completamente por meio de configurações YAML declarativas fornecidas pelo usuário. Operando estritamente no user-space, ela é feita pra ser usada com dotfiles e funciona perfeitamente com ambientes Linux imutáveis/atômicos.

## Features
- **Baseado em Dados:** Todos os apps e suas estratégias de atualização são definidos em arquivos YAML.
- **Resolvedores de Atualização:** Busca a versão mais recente via Forge Releases (GitHub/GitLab), Links Diretos (Cabeçalhos HTTP ETag/Last-Modified) ou Scripts Shell Personalizados.
- **Atualizações Delta:** Usa o backend `zsync-rs` integrado pra baixar só os bytes modificados quando uma receita de app habilita isso.
- **Downloads Segmentados:** Divide downloads diretos grandes em ranges HTTP quando o servidor suporta. Habilitado por padrão.
- **Operações em Paralelo:** `check` e `update` rodam múltiplos apps ao mesmo tempo pra manter lotes grandes rápidos, com limites cientes do provider pra não sobrecarregar o mesmo host.
- **Cooldowns de Rate-Limit:** Apps que batem em rate limits são pulados até o tempo de retry, a menos que você desative isso.
- **Fallback de Proxy GitHub:** Suporte opcional a proxy de metadados do GitHub pode contornar rate limits da API do GitHub sem proxyar o download real, e pode tentar múltiplos bases de proxy em ordem.
- **Integração com Desktop:** Extrai os manifests `.desktop` exatos e ícones diretamente da AppImage usando `--appimage-extract` e insere eles de forma seamless no seu menu de aplicativos em `.local/share/applications`.
- **Verificações de Saúde Locais:** `doctor` verifica a configuração local, diretórios necessários e outros problemas de setup local.
- **Configurações Globais e Locais:** Sobrescreve caminhos de armazenamento, comportamentos de integração, symlinking, downloads segmentados, cooldowns de rate-limit e configurações de proxy GitHub por app ou globalmente.

## Fatos do Projeto
- Esse foi feito pra mim mesmo porque eu tava cansado de atualizar meus AppImages manualmente e queria uma ferramenta que fizesse isso automaticamente sem deletar meus arquivos de config.
- Contribuições são bem-vindas, mas lembre que o projeto é feito pra ser simples, qualquer correção de bug é bem-vinda, features fora do escopo não vão ser adicionadas.
- É intencional que nunca vai ter um repositório pra receitas, os usuários precisam estar confortáveis criando as próprias receitas.
- É só um binário standalone que você pode usar como quiser fora do serviço systemd.
- Nunca vai ter uma GUI, é só uma ferramenta CLI.

## Instalação

### 1. Fedora / OpenSUSE (COPR)
Se você estiver numa distro baseada em RPM, a melhor forma de integrar o `fp-appimage-updater` é pelo repositório COPR oficial.

```bash
sudo dnf copr enable fptbb/fp-appimage-updater
sudo dnf install fp-appimage-updater
```

### 2. Script de Instalação Rápida Universal
Pra todas as outras distribuições Linux (inclusive as atômicas/imutáveis), você pode instalar o binário standalone de forma seamless e configurar os timers systemd em background usando o script de instalação nativo. 

```bash
# Instalação padrão no escopo do usuário (~/.local/bin/ e ~/.config/systemd/user/)
curl -sL fau.fpt.icu/i | bash
```

Se você NÃO quiser o checker automático em background do `systemd` instalado, pode adicionar `--no-systemd`:
```bash
curl -sL fau.fpt.icu/i | bash -s -- --no-systemd
```

Pra instalar o binário e os serviços **system-wide** de forma estrita (apontando pra `/usr/bin/` e `/usr/lib/systemd/system/`), você precisa elevar explicitamente a execução. *(Nota: Se o seu ambiente ativo for estritamente imutável, o script vai rejeitar esse pedido de forma segura).*
```bash
curl -sL fau.fpt.icu/i | sudo bash -s -- --system
```

Pra desinstalar o updater, seus binários e desabilitar de forma graciosa os timers DBus em execução em qualquer escopo:
```bash
curl -sL fau.fpt.icu/i | bash -s -- --uninstall
```

### 3. Usando Binários Pré-compilados
Você pode baixar os binários compilados mais recentes da página oficial de [Releases](https://gitlab.com/fpsys/fp-appimage-updater/-/releases).
Joga o binário limpo na sua pasta de binários preferida (ex: `~/.local/bin/`), roda `chmod +x` e pronto. Ele funciona nativamente como um executável isolado e standalone capaz de se integrar em workflows POSIX padrão, até o self-update funciona.

### Compilando do Código Fonte
Se você quiser compilar a ferramenta você mesmo a partir da árvore de fontes, por favor revise as guidelines em [CONTRIBUTING](CONTRIBUTING.md).

## Documentação

A documentação completa fica em [docs.fau.fpt.icu](https://docs.fau.fpt.icu/). Ela cobre o fluxo de setup passo a passo, formato das receitas, estratégias de atualização, solução de problemas e os detalhes de baixo nível que são mais fáceis de manter num site dedicado de docs do que num README curto.

Se você tá tentando entender como um comando se comporta ou por que um app foi pulado, começa por lá primeiro.

## Seções da Documentação:
*clique pra expandir*
<details>
<summary>1. Estrutura de Diretórios / Configuração</summary>

### A ferramenta espera receitas de aplicativos na sua pasta `~/.config/fp-appimage-updater/`.

```
~/.config/fp-appimage-updater/
├── config.yml                # Comportamentos globais (caminhos de armazenamento, symlinks, toggles de integração)
└── apps/                     # Seus aplicativos
    ├── hayase/
    │   ├── app.yml           # Definição pro Hayase
    │   └── resolver.sh       # Script de parsing personalizado se a Strategy for 'script'
    └── whatpulse.yml         # Definição usando a Strategy 'direct' via ETags
```

### Exemplo de Configuração Global (`config.yml`)
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
  - "https://gh-proxy.com/"
  - "https://corsproxy.io/?"
  - "https://api.allorigins.win/raw?url="
```

### Exemplo de Receita de App (`apps/whatpulse.yml`)
```yaml
name: whatpulse
strategy:
  strategy: direct
  url: "https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage"
  check_method: etag
segmented_downloads: true
```

### Atualizações Delta com Zsync
`zsync` é um caminho opcional de download delta por app, alimentado pelo backend `zsync-rs` integrado. Ele só roda quando a receita inclui um campo `zsync` e o updater consegue encontrar tanto uma AppImage instalada existente quanto um manifesto `.zsync` correspondente.

Formas suportadas na receita:
- `zsync: true` significa que o updater vai tentar `<resolved-download-url>.zsync`
- `zsync: "https://example.org/file.AppImage.zsync"` significa que o updater vai usar aquela URL exata do manifesto

Se o update delta falhar por qualquer motivo, o updater imprime um aviso e volta pro caminho normal de download HTTP.

Exemplo:
```yaml
name: my-app
strategy:
  strategy: forge
  repository: https://github.com/example/my-app
  asset_match: "my-app-*-x86_64.AppImage"
zsync: true
```

### Estratégias de Atualização

fp-appimage-updater suporta três estratégias diferentes pra resolver e baixar atualizações.

#### 1. forge
Usado pra baixar de releases do GitHub ou GitLab.
- `repository`: A URL pro repositório do GitHub ou GitLab.
- `asset_match`: Uma string com wildcard pra combinar o nome específico do asset na release (ex: `"*-amd64.AppImage"`).
- `asset_match_regex`: Matcher de regex opcional pro nome do arquivo do asset. Use isso quando um glob combinaria com assets demais na release. O regex é comparado contra o nome completo do asset.
- `github_proxy`: Fallback opcional de proxy só pra metadados do GitHub por app. Quando habilitado, o `fp-appimage-updater` tenta de novo a API de release do GitHub pelos bases de proxy configurados se o request direto bater em rate limit. O download final ainda usa a URL direta do asset do GitHub.
- `github_proxy_prefix`: Base URL de proxy opcional, array de URLs base, ou a string `all` usada quando `github_proxy` tá habilitado. Padrão é `https://gh-proxy.com/`. O app tenta elas em ordem até uma funcionar. Use `all` pra tentar todo proxy compatível embutido no app.
- `respect_rate_limits`: Override opcional por app que diz pro updater pular apps até a janela de retry expirar quando bater em rate limit. Padrão é `true`.

Pra repositórios do GitLab, o resolver forge usa a API permalink latest em `https://gitlab.com/api/v4/projects/<project-path>/releases/permalink/latest`, lê `assets.links` e prefere `direct_asset_url` quando disponível.

**Exemplo:**
```yaml
strategy:
  strategy: forge
  repository: https://github.com/hydralauncher/hydra
  asset_match: "hydralauncher-*.AppImage"
segmented_downloads: true
```

**Exemplo de edge-case com Regex:**
```yaml
name: obsidian
strategy:
  strategy: forge
  repository: "https://github.com/obsidianmd/obsidian-releases"
  asset_match_regex: "^Obsidian-[0-9.]+\\.AppImage$"
```

Esse regex combina com `Obsidian-1.12.7.AppImage` e evita o asset `Obsidian-1.12.7-arm64.AppImage`.

#### 2. direct
Usado quando o aplicativo fornece uma URL de download direto que sempre aponta pra versão mais recente.
- `url`: A URL estática de download.
- `check_method`: Como detectar se o arquivo remoto mudou. Use `etag` ou `last_modified`.
- `segmented_downloads`: Override opcional por app pra downloads com range HTTP. Quando não setado, usa o flag global `segmented_downloads` que padrão é `true`.

**Exemplo:**
```yaml
strategy:
  strategy: direct
  url: "https://releases.whatpulse.org/latest/linux/whatpulse-linux-latest_amd64.AppImage"
  check_method: etag
segmented_downloads: true
```

#### 3. script
Usado pra cenários complexos onde você precisa rodar um script bash customizado pra determinar a URL de download mais recente e um identificador de versão local pra comparar. O script deve outputar duas linhas: a URL de download na primeira linha, e a string de versão única na segunda linha.
- `script_path`: O caminho relativo pro script bash local.

**Exemplo:**
```yaml
strategy:
  strategy: script
  script_path: ./resolver.sh
segmented_downloads: true
```

Mais exemplos na pasta [examples/apps/](examples/apps/).
</details>
<br />
<details>
<summary>2. Atualizações em Background com Systemd</summary>
<br />
Se você instalou o app usando o script de instalação rápida, um timer do systemd é configurado automaticamente pra rodar checks periodicamente em background.

Como essa ferramenta é projetada estritamente em torno de operações no user-space, **não use `sudo`** quando interagir com os serviços systemd dela (exceto se você instalou system-wide, nesse caso você deve usar `sudo` e a flag `--system` em vez de `--user`).

Verifique o status do timer em background:
```bash
systemctl --user status fp-appimage-updater.timer
```

Ver logs da execução mais recente em background:
```bash
journalctl --user -u fp-appimage-updater.service -n 50
```

Ative ou inicie o timer manualmente:
```bash
systemctl --user enable --now fp-appimage-updater.timer
```
</details>
<br />
<details>
<summary>3. Uso da CLI</summary>

### Saída JSON
Adicione `--json` pra `init`, `validate`, `doctor`, `list`, `check`, `update` ou `remove` quando quiser saída legível por máquina em vez de tabelas e linhas de status.

### Inicializar Configuração
Crie arquivos de configuração iniciais pro config global ou uma receita de app específica:
```bash
fp-appimage-updater init --global
```

Crie um scaffold de receita de app com uma estratégia de update escolhida:
```bash
fp-appimage-updater init --app whatpulse --strategy direct
```

Use `--force` pra sobrescrever arquivos existentes se precisar.

### Validar Receitas
Valide todos os arquivos de receita de aplicativos configurados:
```bash
fp-appimage-updater validate
```

Valide uma única receita pelo nome do app:
```bash
fp-appimage-updater validate whatpulse
```

Esse comando verifica se os arquivos de receita fazem parse corretamente e reporta arquivos inválidos pra você corrigir antes de rodar updates.

### Doctor
Rode uma verificação rápida de saúde no setup local:
```bash
fp-appimage-updater doctor
```

Esse comando verifica:
- o diretório de config
- o diretório de apps
- o arquivo de config global
- o diretório de state
- se o lock do processo tá faltando, ativo ou stale
- se alguma receita de arquivo foi parseada com sucesso
- se alguma receita de arquivo falhou no parse
- se o setup local parece são pras operações de update

Verifique o status de todas as suas receitas configuradas pra ver se tem novas versões disponíveis remotamente:
```bash
fp-appimage-updater check
```

Verifique um único app:
```bash
fp-appimage-updater check whatpulse
```

A saída do `check` agora também reporta hints de suporte quando disponíveis, como suporte a range pra downloads diretos segmentados e os metadados do resolver usados pra comparar versões.

### Atualizar Aplicativos
Instale ou atualize uma única AppImage:
```bash
fp-appimage-updater update whatpulse
```

Atualize todas as configurações de uma vez:
```bash
fp-appimage-updater update
```

Atualizações bem-sucedidas agora incluem o tempo decorrido em segundos pra você ver quanto tempo cada app levou pra instalar ou atualizar.
Quando o updater detecta um rate limit, ele lembra a janela de retry e pula aquele app na próxima rodada a menos que `respect_rate_limits` esteja desabilitado globalmente ou pro app.
Apps forge do GitHub podem opcionalmente usar `github_proxy` com uma string custom `github_proxy_prefix` ou array pra retry lookups de metadados por um ou mais proxies sem proxyar a URL de download real.
Downloads são agendados com um pequeno cap ciente do provider, então o updater continua andando sem sobrecarregar um único host.

### Listar Apps Instalados
Revise a integração atual e as versões locais dos seus aplicativos definidos:
```bash
fp-appimage-updater list
```

### Remover Aplicativos
Remove o binário de um app, symlink, ícones extraídos e arquivos desktop:
```bash
fp-appimage-updater remove whatpulse
```

Remove todos os aplicativos instalados de uma vez:
```bash
fp-appimage-updater remove -a
```
</details>