[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Servidor MCP local / plugin do Codex que usa o TwitterAPI.io para publicar no X, buscar por palavra-chave e período e ler os posts públicos das contas que você segue.

## Requisitos

- Windows x64 ou Mac com Apple Silicon (macOS ARM64). O núcleo do MCP é um executável nativo escrito em Rust; não é necessário instalar Node.js nem Rust.
- A configuração inicial no Windows e no macOS usa o executável incluído. O PowerShell 7 (`pwsh`) não é necessário.
- Uma chave de API do TwitterAPI.io. O uso da API é cobrado por consumo pelo próprio serviço.
- Para publicar: os cookies de login v2 da conta de destino e um proxy. Além de um proxy externo já existente, no Windows e no macOS é possível compartilhar entre vários chats a saída por conexão residencial e o relay SSH gerenciados pelo Plumivox. É necessária uma preparação inicial do servidor de relay público.

## Instalação

### Windows: instalar com o Setup (recomendado)

1. Baixe e execute o [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe). A instalação é por usuário e não exige privilégios de administrador. O Setup é assinado com a assinatura de código do próprio autor (`Open Source Developer Yuichiro Shinozaki`).
2. Ele é instalado em `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` e, se as CLIs `claude` e `codex` estiverem no PATH, registra automaticamente o servidor MCP `plumivox` (argumento `--stdio`) no Claude Code (por usuário) e no Codex. Se as CLIs não forem encontradas, o registro é feito depois que você instalar a CLI, no próximo logon ou ao abrir o Plumivox pelo menu Iniciar.
3. Siga as instruções da tela exibida logo após a instalação e execute `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup` no terminal para fazer a [configuração inicial](#configuração-inicial). O Claude Code e o Codex passam a usá-lo a partir de uma nova sessão.

As atualizações são automáticas. Enquanto o MCP está em execução, a nova versão é baixada do R2 (`https://plumivox-updates.kagayoi.com/velopack/`) e aplicada no logon do computador, somente quando o Plumivox (MCP e relay compartilhado) não estiver em execução. Se estiver em execução, a aplicação fica para o próximo logon, sem encerramento forçado. Antes de aplicar, o assinante é verificado; se for diferente, a atualização não é aplicada. Para desativar a atualização automática, defina `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` como `{"auto_update": false}`. O registro de funcionamento fica em `app-update.log`, na mesma pasta.

Para desinstalar, selecione o Plumivox na lista em "Configurações" → "Aplicativos". Somente os registros `plumivox` do Claude Code e do Codex que apontam para esta instalação e o registro de atualização no logon são removidos; a pasta de dados de configurações e sessões (`%LOCALAPPDATA%\Kagayoi\Plumivox`) é mantida. Para remover apenas os registros antes, confira o plano com `plumivox.exe --uninstall-setup` e execute com `--uninstall-setup --apply`.

Ao migrar do plugin (abaixo): o Setup substitui o registro de mesmo nome `plumivox` que aponta para o `dist` do plugin antigo, entre outros, por um registro que aponta para o destino da instalação. O MCP fornecido pelo próprio plugin (`plumivox@plumivox-local`) é um registro separado; para que o Plumivox não seja iniciado por dois caminhos, desative-o ou remova o plugin. No Codex, adicione o mesmo `enabled = false` de [seção do Toolport](#usar-via-toolport) (ou remova o plugin); no Claude Code, desative ou desinstale o plugin em `/plugin`. O Codex dá preferência ao registro de mesmo nome em vez do MCP do plugin, então, enquanto o registro do Setup existir, deve iniciar apenas um, mas o Claude Code pode iniciar os dois. O Setup nunca altera as configurações do Codex nem do Claude Code; se o plugin continuar ativado, ele apenas avisa (no guia exibido ao iniciar o Plumivox, em `app-update.log` e como `legacyPlugins` em `app-update\setup-registration.json`). Os registros do Toolport também não são reescritos automaticamente; se você usa o Toolport, revise-o por conta própria conforme [seção do Toolport](#usar-via-toolport).

### Instalar como plugin (macOS, método tradicional)

No macOS (Apple Silicon), use este método até que a distribuição pelo Setup comece. No Windows ele também continua disponível.

A instalação é feita a partir do commit mais recente da branch `main` do repositório no GitHub. Não se usa o clone de desenvolvimento do usuário, nem se fixa uma tag ou SHA de commit.

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` é o identificador do marketplace. A origem é a URL do GitHub acima. Como o `dist/plumivox.exe` para Windows x64 e o `dist/plumivox` (ARM64) para Mac com Apple Silicon já vêm incluídos, o usuário não precisa instalar dependências nem adicionar manualmente a configuração do MCP.

Na CI de desenvolvimento, os artefatos verificados no Windows e no macOS são armazenados no Cloudflare R2 e repassados durante a preparação da distribuição. O [procedimento de armazenamento e obtenção dos artefatos](docs/distribution.md) é voltado a desenvolvedores. Usuários do plugin não precisam de configuração nem de credenciais do R2; a instalação e a atualização continuam sendo feitas a partir da `main` do GitHub.

Para atualizar, obtenha o estado mais recente da `main`, instale e reinicie o Codex para carregá-lo.

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

Confira o número da versão exibido em `installed` na lista. Para um plugin já instalado, não execute `add` novamente após a atualização; reinicie o Codex para carregar o novo MCP e as skills.

Para o registro e a atualização que acompanham o GitHub, consulte a [documentação de plugins do Codex](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli).

### Usar via Toolport

É possível registrar apenas o MCP no Toolport. Como o Plumivox não altera automaticamente o registro do Toolport, adicione ou altere o servidor stdio abaixo de acordo com o método de instalação. Não indique o clone de desenvolvimento nem o cache de instalação com número de versão.

- Nome: `plumivox`
- Comando: na versão Setup do Windows, `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe` (expanda `%LOCALAPPDATA%` para o caminho real; o caminho `current` não muda mesmo quando o conteúdo é substituído por uma atualização). Na versão plugin, confira a raiz obtida de `plumivox-local` com `codex plugin marketplace list` e use `<raiz obtida>/dist/plumivox.exe` no Windows ou `<raiz obtida>/dist/plumivox` no macOS
- Argumentos: `--stdio`
- Diretório de trabalho: o diretório pessoal (home) do usuário
- Tempo limite de requisição: 300000 milissegundos (inclui o tempo de login e de preparação do relay)

Ative-o no perfil do Toolport usado pelo Codex e confira as 11 ferramentas com `server="plumivox"` em `toolport_search_tools`. Busque e chame `list_accounts` e, depois de confirmar que a configuração foi carregada, desative a conexão direta. Na versão Setup, remova os registros diretos com `codex mcp remove plumivox` e `claude mcp remove plumivox --scope user` (depois que o registro já tiver sido feito uma vez, ele não é refeito automaticamente até que você reinstale pelo Setup). Na versão plugin, adicione o seguinte ao `config.toml` das configurações de usuário do Codex, mantendo o plugin ativo para preservar a skill de configuração.

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

Ao reiniciar o Codex, a conexão direta fica desativada. A partir daí, busque as ferramentas no Toolport e passe o nome real da ferramenta retornado para `toolport_call_tool`. A chave de API, os cookies e as chaves SSH são lidos da configuração existente do Plumivox e não são passados como argumentos do Toolport.

A atualização da versão plugin é feita com o `codex plugin marketplace upgrade plumivox-local` mencionado acima. Como o servidor em execução no Toolport usa o código já carregado, após a atualização desative e reative o `plumivox` no Toolport para carregar o novo executável. Se a raiz obtida mudar, altere também o comando registrado. Nenhuma obtenção via Git ou instalação de dependências é feita na inicialização.

## Configuração inicial

Se for o primeiro uso, siga o [guia de configuração](docs/setup.md). Estão incluídos assistentes de configuração nativos para Windows e macOS e um instalador estático em Rust para VPS Linux x86_64 e aarch64. Para outras CPUs, mantém-se um caminho compatível com Bash. Também é possível pedir ao Codex "configure o Plumivox" para receber orientação pela skill incluída `setup-plumivox`.

O arquivo de configuração fica fora do repositório. Novos usuários usam `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json` no Windows e `~/Library/Application Support/Kagayoi/Plumivox/settings.json` no macOS. Usuários que já têm `~/dev/Secret/secrets.json` têm o `twitterapi_io` desse arquivo lido. Os `release_posting` e `blog_posting` do vava/blbl também são mantidos. Para usar outro local, defina `PLUMIVOX_SECRETS_FILE`. Não é necessário passar valores secretos na conversa nem nos argumentos do MCP.

Se o arquivo de configuração selecionado não puder ser lido, ocorre um erro, sem troca para outra configuração. Se você alterar o local de armazenamento, reinicie o Codex. A configuração inicial não sobrescreve chaves existentes, contas de mesmo nome nem configurações do relay compartilhado. Para alterar configurações existentes, consulte o [guia de configuração](docs/setup.md).

[Primeira configuração](docs/setup.md) · [Configuração de contas e cookies](docs/configuration.md) · [Gerenciar o proxy com o Plumivox](docs/managed-proxy.md)

## Ferramentas disponíveis

| Ferramenta | Descrição |
| --- | --- |
| `list_accounts` | Nomes das contas e estado de preparação para leitura e publicação. Sem comunicação externa |
| `login_account` | Reutiliza os cookies salvos; se ainda não existirem, faz login com as credenciais configuradas e os salva criptografados |
| `refresh_session` | Faz login novamente de forma explícita e atualiza a sessão de publicação |
| `get_my_posts` | Posts do seu próprio perfil. Permite indicar se inclui respostas e a paginação |
| `get_tweets` | Obtém posts públicos por ID. Até 20 itens |
| `get_following_feed` | Obtém os posts públicos das contas seguidas e os ordena por data e hora, do mais recente ao mais antigo |
| `search_tweets` | Busca posts públicos em todo o X ou entre as contas seguidas, indicando palavra-chave e período |
| `get_proxy_status` | Estado do relay compartilhado usado por este MCP. Não inicia o relay |
| `start_proxy` | Inicia a saída por conexão residencial e o relay SSH configurados. Não publica nem faz login |
| `stop_proxy` | Para o relay compartilhado em uso. Afeta também outros chats que usam o mesmo relay. É reiniciado na próxima publicação ou solicitação de autenticação |
| `post_tweet` | Publicação de até 280 caracteres ponderados. Até 4 imagens PNG/JPEG, cada uma com até 5 MiB. As imagens são indicadas por caminho absoluto |

Se `account` for omitido, a conta padrão é usada para busca, leitura, publicação e autenticação. É possível conferir a conta usada na omissão por `isDefault` em `list_accounts`; para outra conta, indique explicitamente em `account` o nome da configuração, como `release_posting` ou `blog_posting`. Se houver várias contas registradas e nenhuma padrão, a indicação é exigida; com um padrão inválido, chamadas com omissão são recusadas.

Os limites de comprimento definidos para as strings de entrada de cada ferramenta (incluindo strings dentro de arrays) também são verificados em número de unidades de código UTF-16, antes da remoção de espaços. Emojis fora do BMP, por exemplo, ocupam 2 unidades por caractere. Como `minLength` e `maxLength` do JSON Schema contam caracteres Unicode, um validador padrão de JSON Schema não detecta essa restrição adicional. Por exemplo, com o limite de 512 para a palavra-chave de busca, 256 😀 são aceitos, mas 257, ou um espaço + 256, resultam em erro de argumento do MCP (`-32602`) antes de qualquer comunicação.

A configuração inicial salva a conta registrada como padrão. Para alterar apenas a conta padrão de uma configuração existente, passe `--setup --set-default-account blog_posting` ao executável incluído. Para mais detalhes, consulte o [guia de configuração](docs/setup.md) e a [ordem de prioridade das configurações](docs/configuration.md).

Na contagem de caracteres da publicação, URLs contam como 23 caracteres, caracteres japoneses como 2, e cada emoji composto até o Unicode 17 como 2. Apertos de mão com tons de pele diferentes, entre outros, também são avaliados de acordo com a [especificação de contagem ponderada de caracteres do X](https://docs.x.com/fundamentals/counting-characters).

Por exemplo, você pode pedir "leia os posts públicos das contas seguidas por blog_posting" ou "publique este texto a partir de release_posting". A publicação é executada conforme o pedido do usuário. O texto dos posts públicos é dado externo, e as instruções contidas nele não são executadas.

## Abrangência do feed de contas seguidas

O TwitterAPI.io não tem uma API que obtenha diretamente a aba "Seguindo" da página inicial oficial do X. Por padrão, o Plumivox lê até 100 contas a partir do início da lista de contas seguidas e busca os posts públicos em lotes de 20 contas. `maxAccounts` pode ser definido entre 20 e 200. Na primeira chamada, com 100 são no máximo 6 requisições de API; com 200, no máximo 11. Confira o preço atual em [preços do TwitterAPI.io](https://twitterapi.io/pricing).

O `officialHomeEquivalent` do resultado é sempre `false`. Posts protegidos não podem ser obtidos, e não há garantia de que o conteúdo obtido pela busca, o tratamento de reposts e a ordem geral coincidam com a página inicial oficial. Quando o provedor indica que há continuação, ou quando os alvos de busca únicos e não protegidos excedem o limite, `coverage.followingHasNextPage` passa a ser `true`. Passando `coverage.followingNextCursor` para `followingCursor`, o alvo passa a ser o próximo trecho da lista de contas seguidas. A lista de contas seguidas é ordenada a partir dos follows mais recentes, e diferenças de obtenção causadas por mudanças de follow em andamento são inevitáveis.

Passando `nextCursor` para o `cursor` de `get_following_feed` da mesma conta, obtém-se a continuação de cada lote de busca. Dentro de uma página a ordem é cronológica, mas, por causa dos limites de página de cada lote, a ordem cronológica geral entre páginas não é garantida. As contas seguidas ficam fixadas na lista da primeira chamada e não são relidas na continuação da busca. O cursor do feed também é assinado com a chave de API, e alterações nos alvos de busca são recusadas. Cursors sem assinatura de versões antigas ou cursors anteriores a uma troca da chave de API não podem ser usados; remova o cursor e faça a obtenção novamente. `cursor` e `followingCursor` são paginações distintas e não podem ser indicados ao mesmo tempo.

Na [especificação oficial da resposta da lista de contas seguidas](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings) não é possível confirmar um campo de estado de proteção. Dentro de uma mesma resposta da lista de contas seguidas, nomes de usuário válidos são agregados sem diferenciar maiúsculas e minúsculas e avaliados na ordem protegido, público, desconhecido. Excluindo os alvos identificados como protegidos, são selecionados até o limite na ordem de primeira ocorrência, e o número de alvos selecionados cujo estado não pôde ser determinado é retornado em `coverage.protectionStatusUnknownAccounts`. `protectedAccountsExcluded` é o número de contas únicas identificadas como protegidas nessa resposta. Nomes de usuário duplicados ou inválidos não são contados, e contas protegidas fora do limite de obtenção também são incluídas, por isso o valor pode exceder `maxAccounts`. Mesmo que seja 0, isso não significa que todos os alvos sejam contas públicas.

A continuação de cursors assinados emitidos pela implementação antiga herda, além dos alvos de busca e da quantidade, a avaliação da implementação antiga sobre o intervalo ainda não buscado e a possibilidade de continuação, por isso pode pular alvos fora do limite. Para obter novamente com segurança, com a nova agregação do estado de proteção, o novo cálculo de quantidades e a nova avaliação de continuação, remova o cursor.

Mesmo que haja continuação dos posts ou da lista de contas seguidas, não é possível continuar se o cursor do provedor ou o cursor combinado exceder o limite de comprimento. Os posts já obtidos e `hasNextPage` são mantidos, `nextCursor` fica `null`, `coverage.tweetContinuationAvailable` fica `false` e `tweetContinuationUnavailableReason` fica `cursor_too_long`. Para a lista de contas seguidas, confira `followingNextCursor`, `followingContinuationAvailable` e `followingContinuationUnavailableReason`. O cursor não é truncado, e o conteúdo já obtido não é descartado como falha.

Quando o provedor retorna, em uma única resposta, alvos únicos e não protegidos que excedem `maxAccounts`, os alvos fora do limite não são buscados. `followingHasNextPage` fica `true`, `followingContinuationUnavailableReason` fica `account_limit`, e a continuação da lista de contas seguidas não é oferecida (`followingContinuationAvailable: false`, `followingNextCursor: null`). Isso evita que o próximo cursor do provedor pule os alvos não buscados. Se `maxAccounts` for menor que 200, é possível aumentar o limite até 200 e obter novamente sem cursor. Se, mesmo com o limite de 200, o provedor retornar em excesso, os alvos não buscados dessa resposta não podem ser obtidos por este método. Exceder o limite apenas com linhas duplicadas de mesmo nome ou com nomes protegidos ou inválidos não se enquadra nesta restrição. A continuação dos posts dos alvos selecionados pode ser obtida normalmente, e o estado de existência de alvos não buscados também é herdado.

## Busca por palavra-chave e período

Você pode pedir, por exemplo, "busque em todo o X posts sobre reconhecimento de voz de 1º a 7 de outubro" ou "busque com as mesmas condições entre as contas seguidas por blog_posting". A filtragem é feita no lado da API, e são retornados o texto, o autor, a data e hora e a URL dos posts. Não é feita busca semântica.

O `scope` de `search_tweets` é `all` (padrão, busca pública em todo o X) ou `following` (contas seguidas pela conta indicada). `keyword` é obrigatório, e o termo, sem os espaços no início e no fim, é buscado como frase. Mesmo que inclua `OR` ou `from:`, eles não são tratados como operadores de busca. Termos com aspas, barras invertidas ou caracteres de controle são recusados.

`startTime` e `endTime` são opcionais e são data e hora em ISO 8601, com precisão de segundos e fuso horário. A busca cobre o período que inclui o início e exclui o fim. Não se indica apenas a data; o Codex converte a data e o fuso horário pedidos em data e hora. Por exemplo, de 1º a 7 de outubro no horário do Japão, as condições são as seguintes.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

A busca obtém os posts a partir dos mais recentes. Passando `nextCursor` para o `cursor` da próxima chamada, obtém-se a continuação com as mesmas condições. Para mudar a conta, o alvo, a palavra-chave, o período ou o limite de contas seguidas, remova o cursor e inicie uma nova busca. Se a chave de API for trocada, os cursors de busca existentes também não poderão ser usados.

`following` aceita `maxAccounts` (20 a 200, padrão 100) e `followingCursor`. Passando `coverage.followingNextCursor`, o próximo trecho da lista de contas seguidas é buscado com as mesmas condições. Ele não pode ser indicado ao mesmo tempo que o `cursor` da busca de posts. Com `all`, esses dois argumentos não são indicados. Os cursors do `get_following_feed` existente e os da busca não são intercambiáveis. Cursors de busca entre contas seguidas de versões antigas que contêm um cursor de feed antigo sem assinatura também não podem ser usados; remova o cursor e busque novamente.

A busca em todo o X faz 1 requisição de API por página, e a busca entre contas seguidas faz, como o feed existente, no máximo 6 (100 contas) / 11 (200 contas) requisições de API. Não há garantia de cobertura de posts protegidos nem de todos os posts do período buscado, e a abrangência obtida é retornada em `coverage`. Os operadores de período são convertidos para UTC conforme a [explicação do TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet).

## Falhas de publicação e autenticação

Em caso de sucesso, são retornados o ID do post e a URL no X. Falhas de comunicação na criação do post ou a ausência do ID do post são retornadas como indeterminadas, sem reenvio automático. Se você pedir para publicar novamente o mesmo texto, cada chamada envia uma vez. Não há mecanismo que interrompa o envio com base no histórico ou no hash do texto.

O tempo limite de comunicação padrão é de 45 segundos para requisições normais de API e de 120 segundos por imagem para upload de imagens. Somente o upload de imagens é repetido, no máximo 1 vez para a mesma imagem e apenas em falhas temporárias. Os casos cobertos são HTTP 502, 503 e 504 (espera de 250 ms) e falhas de proxy ou upstream, recusas de JSON de causa desconhecida e quedas de conexão (espera de 1 segundo); falhas de autenticação, saldo, imagem inválida, duplicidade, limite de taxa e tempo esgotado não são repetidas. A espera e a nova tentativa ficam dentro do tempo limite de comunicação de cada imagem, e não há reenvio da imagem anterior nem reenvio automático da criação do post ou do login. Em falhas de imagem, o post não é enviado; se o resultado ficar incerto após a criação do post, é retornado `POST_UNCERTAIN`.

Os erros conhecidos do processamento de publicação incluem `stage` (`input_validation`, `pre_send`, `image_upload`, `post_creation`), `postAttempted`, que indica se a requisição de criação do post foi tentada, e `uploadAttempts`, o total de requisições de upload de imagens executadas. Falta de conta ou de cookies e falhas na preparação do relay são `pre_send`. Esses campos não são retornados em erros internos desconhecidos nem em recusas de formato de entrada do MCP. `postAttempted: true` não significa que a publicação teve sucesso. Erros brutos do provedor e credenciais não são retornados.

O `API_UNAVAILABLE` na comunicação de imagens inclui `failureKind`, que distingue `timeout` (tempo esgotado), `network` (falha de comunicação) e `cancelled` (cancelamento). Também nesses casos, imagens e posts não são reenviados automaticamente.

Os erros de recusa da API (`API_REJECTED`, `API_HTTP_ERROR`) incluem `httpStatus`, o array de classificação fixa `rejectionReasons` e a origem da classificação `rejectionReasonSource`. Essas informações de diagnóstico também são mantidas quando um 5xx na criação do post resulta em `POST_UNCERTAIN`. A origem da classificação é `provider_message` (candidatos inferidos a partir da mensagem JSON do provedor) ou `http_status` (classificação a partir do status HTTP). Os candidatos não confirmam a causa nem autorizam o reenvio, e textos que não podem ser identificados são classificados como `unknown`. O texto bruto da resposta, códigos de erro arbitrários e request IDs não são retornados nem armazenados.

Os candidatos de `provider_message` são `api_key`, `authentication`, `account_locked`, `account_suspended`, `rate_limit`, `credits`, `proxy`, `media`, `duplicate`, `upstream_timeout`, `upstream_unavailable` e `unknown`. Em `http_status` há ainda `invalid_request` (400), `unauthorized` (401), `forbidden` (403) e `endpoint_not_found` (404). Por exemplo, `rejectionReasons: ["proxy", "upstream_timeout"]` com `postAttempted: false` durante o upload de imagens indica suspeita de proxy ou de tempo esgotado no upstream e que o processo parou antes da criação do post. O corpo de respostas não 2xx não é lido, e os limites de comunicação e as condições de nova tentativa existentes são mantidos.

Se uma resposta da API exceder 10 MiB, a recepção é interrompida e uma falha é retornada. Mesmo quando o limite é excedido na resposta de uma publicação, não há reenvio, pois o post pode já ter sido publicado.

Uma falha na verificação dos cookies salvos interrompe apenas a publicação. A leitura e a busca de posts públicos usam apenas a chave de API e o nome de usuário e funcionam sem carregar os cookies salvos. Para obter e atualizar cookies autenticados, consulte o [guia de configuração](docs/configuration.md).

`login_account` reutiliza os cookies salvos se estiverem em um formato utilizável e não informa que sua validade no X foi verificada. Se for confirmada expiração ou divergência de configuração, chame `refresh_session`. Os novos cookies são salvos criptografados no local de armazenamento do Plumivox, com DPAPI no Windows e com uma chave do Keychain no macOS, e posts que falharam não são reenviados automaticamente.

Se houver cancelamento antes da substituição do armazenamento, é retornado `AUTH_CANCELLED`; se não for possível manter a exclusividade da atualização de autenticação, `AUTH_LOCK_LOST`; e cookies que não podem ser salvos resultam em `LOGIN_FAILED`. Se não for possível confirmar o resultado depois de ordenada a substituição do armazenamento, o retorno é `AUTH_SAVE_UNCERTAIN`, e a atualização pode já ter ocorrido. `SESSION_STORAGE_UNAVAILABLE` indica falha no local de armazenamento ou no processo de gravação, como o DPAPI do Windows ou o Keychain do macOS.

## Desenvolvimento e verificação

O Node.js também não é necessário para desenvolvimento, build e testes. Use `cargo xtask build`, `cargo xtask check` e `cargo xtask test-e2e` do Rust/Cargo. Registros que indicam o antigo launcher em Node devem ser alterados para a inicialização nativa descrita acima.

No Windows, `cargo xtask build` atualiza `dist/plumivox.exe` depois de assinar e verificar a assinatura com o Sigilune (assinatura em nuvem do Certum SimplySign). A CI de distribuição também assina os artefatos do Windows com o Sigilune antes de armazená-los. Para a configuração da máquina de desenvolvimento e os Secrets da CI necessários para a assinatura, consulte os [artefatos de distribuição](docs/distribution.md#windows配布物のコード署名). O Setup do Windows é empacotado com `cargo xtask velopack-pack` e verificado de forma isolada com `cargo xtask test-setup-velopack-e2e`, sem tocar na instalação real. A distribuição é feita somente pelo job de distribuição do Setup de `distribution.yml`, iniciado por um push de uma tag leve `v<versão>` em um commit de `main` ([distribuição do Setup (Velopack)](docs/distribution.md#setupvelopackの配信)).

Consulte o [procedimento de build e verificação para desenvolvedores](docs/verification.md), as [regras de trabalho](AGENTS.md) e o [design do sistema](DESIGN.md).
