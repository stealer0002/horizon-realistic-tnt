# Como instalar o Realistic TNT

O Realistic TNT é um mod nativo: código de verdade, compilado para **uma versão exata do Horizon**,
a Nightly de 22/11/2026 (`f1bb66f`). O launcher compila o mod para essa versão e deixa uma instância
presa nela. Assim, quando o Horizon atualizar, o seu mundo com as TNTs continua funcionando.

Use uma instância só para o mod. O seu jogo principal e os seus mundos ficam como estão.

**Você precisa** do launcher do Horizon, versão 0.2.70 ou mais nova ([playhorizon.dev](https://playhorizon.dev)),
com uma conta que tenha o Minecraft: Java Edition.

## 1. Crie uma instância para o mod

No launcher: **Instâncias > Nova instância**, com o nome `Realistic TNT`.

Ou no terminal:

```sh
horizon-launcher instance create "Realistic TNT"
```

## 2. Instale o mod nela

No launcher: escolha a instância `Realistic TNT`, abra **Mods > Adicionar do GitHub**, cole o link
abaixo e permita código nativo:

```
https://github.com/stealer0002/horizon-realistic-tnt/releases/tag/v0.11.0
```

Ou no terminal:

```sh
horizon-launcher mods add-github https://github.com/stealer0002/horizon-realistic-tnt/releases/tag/v0.11.0 --instance "Realistic TNT" --trust-native-code
```

O launcher baixa a Nightly de 22/11/2026 do Horizon se você não tiver, compila o mod para ela (alguns
minutos na primeira vez) e faz a instância jogar essa versão.

Um mod nativo roda com as permissões do seu usuário, por isso o launcher pede que você confie nele.
Todo o código está neste repositório.

## 3. Jogue

Escolha a instância e aperte **Jogar**.

- As TNTs de valor (×2 até ×1.000.000) e o detonador estão na aba do criativo **TNT realista** (**Realistic TNT** com o jogo em inglês).
- Ou use `/give @p realistic_tnt:tnt_x1000` e `/give @p realistic_tnt:detonator`.
- O shader da explosão se instala sozinho: escolha **Realistic TNT: explosions** em
  **Opções > Vídeo > Shaders**.

## Para continuar jogando depois de um update do Horizon

- **A instância fica na versão para a qual o mod foi compilado.** Um update do Horizon é instalado ao
  lado e não mexe nela. Só não recompile essa instância com `--update` ("atualizar os mods para o
  Horizon mais novo").

- **Não deixe o launcher apagar a versão que o mod usa:**

  ```sh
  horizon-launcher settings prune-old-versions off
  ```

- **Se o mod parou de funcionar** (sumiu a aba TNT realista, ou o jogo avisa que um mod ficou de
  fora), a instância foi mudada para outra versão do Horizon. Volte para a versão do mod:

  ```sh
  horizon-launcher versions --installed
  horizon-launcher instance version "Realistic TNT" native-f1bb66f-XXXXXXXXXXXX
  ```

  Troque `native-f1bb66f-XXXXXXXXXXXX` pela linha que começa com `native-f1bb66f-` no resultado do
  primeiro comando.

  Ou compile o mod de novo na versão certa:

  ```sh
  horizon-launcher native-build --instance "Realistic TNT" --version nightly-2026-11-22-f1bb66f --trust-native-code
  ```

- **Quando sair uma versão do mod para um Horizon mais novo**, instale numa instância nova do mesmo
  jeito e mantenha esta até testar a nova.

## Problemas comuns

| O que acontece | O que fazer |
|---|---|
| Não aparece a aba TNT realista | Veja se está jogando na instância `Realistic TNT`, e não na principal. Se estiver, siga "Se o mod parou de funcionar" acima. |
| O launcher pede para confiar em código nativo | É normal para mods nativos. Aceite só se confiar no mod. |
| A explosão grande trava o jogo | Explosões a partir de ×10.000 pesam muito. Teste primeiro com ×100 e ×1000. |
| Não aparece o clarão, a fumaça nem a onda de choque | Escolha o shader **Realistic TNT: explosions** em Opções > Vídeo > Shaders. |
