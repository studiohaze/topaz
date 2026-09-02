# Topaz

Topaz is a programming language for expressing application intent in a compact, consistent syntax. It reads as easily as Python or TypeScript, but instead of scattering one intent across several constructs it gives each idea a single syntactic shape. Identifiers accept Unicode from the lexer onward, so domain terms can be used directly in code without being transliterated into Latin letters.

```topaz
function 글자정리(글: string) -> string {
  if 글 == "&" { "&amp;" }
  else if 글 == "<" { "&lt;" }
  else if 글 == ">" { "&gt;" }
  else { 글 }
}

function 줄정리(줄: string) -> string {
  let mut 출력 = ""
  for 글 in 줄.scalars() {
    출력 = 출력 + 글자정리(글)
  }
  출력
}
```

From [`examples/choseong`](examples/choseong/choseong.tpz). More programs live in [`examples/`](examples/).

Topaz is not aimed at controlling hardware directly. It works one layer up, on command-line tools, data transformation, service logic, configuration handling, and programs that deal with SQL, shells, and file paths. The same source can be run immediately by the interpreter or built into a native program, a Python artifact, or a web artifact.

This repository contains the source of the compiler, the runtime, and the standard library. The compiler is written in Topaz and builds itself. An implementation written in Rust handles bootstrapping and recovery.

## Getting started

On macOS and Linux, install with `curl -fsSL https://topaz.ooo/install.sh | sh`. On Windows PowerShell, use `irm https://topaz.ooo/install.ps1 | iex`. If you have Node.js, `npm install -g topaz-lang` works as well. Run `topaz version` to confirm the installation. The installed binary is self-contained. The compiler, the runtime, and the Lispex evaluator ship in one file.

The language guide, learning path, and standard library reference are at [topaz.ooo](https://topaz.ooo). To try Topaz without installing anything, use the [playground](https://topaz.ooo/playground).

## 5.20

5.20 ships the self-hosted compiler written in Topaz inside the binary, selectable with `--compiler self`. When the selection is omitted, the Rust implementation compiles. The Lispex 1.20 evaluator is built into the binary. The compiler sources have been reorganized into modules small enough to read and maintain.

## Building and testing

Rust 1.96.0 is required. The Python backend tests use CPython 3.13.14. Run `cargo build` and `cargo test` in the `compiler/` directory.

## License

This project is licensed under the Apache License 2.0. See `LICENSE` and `NOTICE` for details.

---

# Топаз

Топаз — язык программирования, который позволяет выражать замысел приложения компактным и единообразным синтаксисом. Код читается так же легко, как на Python или TypeScript, при этом один и тот же замысел не распыляется по нескольким конструкциям, а записывается единой синтаксической структурой. Идентификаторы полностью поддерживают Юникод уже на уровне лексера, поэтому термины предметной области записываются в коде как есть, без транслитерации латиницей.

Топаз не предназначен для прямого управления оборудованием. Он работает уровнем выше, в утилитах командной строки, преобразовании данных, сервисной логике, обработке конфигурации и программах, которые имеют дело с SQL, оболочками и путями к файлам. Один и тот же исходный код можно сразу выполнить интерпретатором или собрать в нативную программу, артефакт для Python или веб-артефакт.

В этом репозитории находятся исходные тексты компилятора, среды выполнения и стандартной библиотеки. Компилятор написан на Топазе и собирает сам себя. Реализация на Rust отвечает за начальную загрузку и восстановление.

## Начало работы

В macOS и Linux установка выполняется командой `curl -fsSL https://topaz.ooo/install.sh | sh`, в Windows PowerShell командой `irm https://topaz.ooo/install.ps1 | iex`. Если установлен Node.js, подойдёт и `npm install -g topaz-lang`. После установки проверьте результат командой `topaz version`. Установленный бинарный файл самодостаточен. Компилятор, среда выполнения и вычислитель Лиспекса поставляются в одном файле.

Описание языка, учебный маршрут и справочник по стандартной библиотеке находятся на [topaz.ooo](https://topaz.ooo). Попробовать Топаз без установки можно в [песочнице](https://topaz.ooo/playground).

## 5.20

В 5.20 самоприменимый компилятор, написанный на Топазе, входит в бинарный файл и выбирается ключом `--compiler self`. Если выбор не указан, компилирует реализация на Rust. Вычислитель Лиспекса 1.20 встроен в бинарный файл. Исходные тексты компилятора разбиты на модули, которые удобно читать и сопровождать.

## Сборка и тестирование

Требуется Rust 1.96.0. Для тестов Python-бэкенда используется CPython 3.13.14. В каталоге `compiler/` выполните `cargo build` и `cargo test`.

## Лицензия

Проект распространяется под лицензией Apache License 2.0. Подробности в файлах `LICENSE` и `NOTICE`.

---

# 토파즈

토파즈는 애플리케이션의 의도를 간결하고 일관된 문법으로 표현하는 프로그래밍 언어입니다. Python이나 TypeScript처럼 읽기 쉬우면서도, 동일한 의도를 여러 구문으로 분산시키지 않고 일관된 문법 구조로 작성할 수 있습니다. 식별자는 렉서 단계부터 유니코드를 온전히 지원하여 도메인 용어를 로마자로 변환할 필요 없이 코드에 직접 사용할 수 있습니다.

토파즈는 하드웨어를 직접 제어하기보다는 그 상위 계층의 작업에 집중합니다. 명령줄 도구, 데이터 변환, 서비스 로직, 설정 처리, 그리고 SQL·셸·파일 경로를 다루는 프로그램이 주된 사용처입니다. 동일한 소스를 인터프리터로 즉시 실행할 수도 있고, 네이티브 프로그램, Python 아티팩트, 웹 아티팩트 등 다양한 형태로 빌드할 수도 있습니다.

이 저장소에는 컴파일러와 런타임, 표준 라이브러리의 소스 코드가 포함되어 있습니다. 컴파일러는 토파즈로 작성되어 자기 자신을 빌드하며, Rust로 작성된 구현체가 부트스트랩과 복구를 담당합니다.

## 시작하기

macOS와 Linux에서는 `curl -fsSL https://topaz.ooo/install.sh | sh`, Windows PowerShell에서는 `irm https://topaz.ooo/install.ps1 | iex`로 설치합니다. Node.js가 있다면 `npm install -g topaz-lang`으로도 설치할 수 있습니다. 설치 후 `topaz version`으로 확인합니다. 설치되는 바이너리는 자체완결형입니다. 컴파일러와 런타임, 리스펙스 평가기가 한 파일에 들어 있습니다.

언어 안내와 학습 경로, 표준 라이브러리 문서는 [topaz.ooo](https://topaz.ooo)에 있으며, 설치 없이 바로 실행해 보려면 [플레이그라운드](https://topaz.ooo/playground)를 이용하세요.

## 5.20

5.20 버전에는 토파즈로 작성된 셀프 호스팅 컴파일러가 바이너리에 실려 있으며 `--compiler self` 로 선택할 수 있습니다. 선택을 생략하면 Rust 구현이 컴파일합니다. 리스펙스 1.20 평가기가 바이너리에 내장되어 있으며, 컴파일러 소스는 가독성과 유지보수를 고려해 적절한 크기의 모듈로 정리되었습니다.

## 빌드와 테스트

Rust 1.96.0이 필요하며, Python 백엔드 테스트에는 CPython 3.13.14를 사용합니다. `compiler/` 디렉터리에서 `cargo build`와 `cargo test`를 실행합니다.

## 라이선스

이 프로젝트는 Apache License 2.0 라이선스를 따릅니다. 자세한 내용은 `LICENSE` 및 `NOTICE` 파일을 참고하세요.
