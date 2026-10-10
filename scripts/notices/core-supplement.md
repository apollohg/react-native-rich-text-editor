## JavaScript, framework, and copied asset notices

## Scope

The published root npm package has no `dependencies` and does not bundle its JavaScript framework packages. Its published sources import React (including `react/jsx-runtime`), React Native, Expo Modules Core, and `@react-native-vector-icons/material-design-icons`; Expo and vector-icons are declared peers. The Expo config plugin resolves `expo/config-plugins` at build time. The optional highlighting package imports Expo Modules Core and the editor's types, with no additional JavaScript library bundled.

Version numbers below describe this checkout's installed packages, not minimum requirements or guaranteed consuming-app versions. Consumers must retain the notices for their actual React Native/Expo/Metro/Hermes native and JavaScript dependency graph and any other icon fonts their app includes. Do not treat the development lockfile as a list of files distributed in the npm package, nor this framework overview as a complete app bundle inventory.

## Material Design Icons assets distributed by this package

`android/src/main/assets/editor-icons/MaterialDesignIcons.ttf` and `MaterialDesignIcons.json` are byte-identical to `fonts/MaterialDesignIcons.ttf` and `glyphmaps/MaterialDesignIcons.json` in `@react-native-vector-icons/material-design-icons` 13.1.4. The package includes Pictogrammers Material Design Icons 7.4.47.

Font SHA-256: `61e8aba5a4e981fe22cf7c8e8bcdbea00476e75c62c37f01bf7ee33361d68428`

Glyphmap SHA-256: `54c620e48800b895a2a6c34ec24e03c56d5c0fdd1625f955ef51bbdb49726c91`

The Pictogrammers Free License identifies fonts as Apache-2.0 and non-font/non-icon code as MIT. Sources: [Material Design Icons 7.4.47 license](https://github.com/Templarian/MaterialDesign-Webfont/blob/v7.4.47/LICENSE), [React Native Vector Icons package](https://github.com/oblador/react-native-vector-icons/tree/master/packages/material-design-icons). Retain the Apache-2.0 text below and the React Native Vector Icons MIT notice when distributing these assets. No other icon fonts are copied into tracked project files.

### Material Design Icons — Apache-2.0

```text

                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
```

## @react-native-vector-icons/material-design-icons

Version: 13.1.4. License: MIT (wrapper and glyphmap); Apache-2.0 (font).

Scope: JS peer and source of copied Android glyphmap/font.

Source: https://github.com/oblador/react-native-vector-icons/tree/master/packages/material-design-icons

Exact local license: `node_modules/@react-native-vector-icons/material-design-icons/LICENSE`.

```text
MIT License

Copyright (c) 2015 Joel Arvidsson
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## React

Version: 19.2.3. License: MIT.

Scope: Framework runtime supplied by host app.

Source: https://github.com/facebook/react

Exact local license: `node_modules/react/LICENSE`.

```text
MIT License

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## React Native

Version: 0.86.2 (root); 0.86.3 (example). License: MIT.

Scope: Framework runtime supplied by host app.

Source: https://github.com/react/react-native

Exact local license: `node_modules/react-native/LICENSE`.

```text
MIT License

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Expo

Version: 57.0.16 (root); 57.0.18 (example). License: MIT.

Scope: Declared peer supplied by host app.

Source: https://github.com/expo/expo

Exact local license: `node_modules/expo/LICENSE`.

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Expo Modules Core

Version: 57.0.13 (root); 57.0.14 (example). License: MIT.

Scope: Framework runtime imported by both editor packages.

Source: https://github.com/expo/expo

Exact local license: `node_modules/expo-modules-core/LICENSE`.

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Expo Font

Version: 57.0.1 (root); 57.0.2 (example). License: MIT.

Scope: Peer of vector-icons used for icon font loading.

Source: https://github.com/expo/expo

Exact local license: `node_modules/expo-font/LICENSE`.

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## @expo/config-plugins

Version: 57.0.9. License: MIT.

Scope: Build-time dependency accessed through expo/config-plugins; not an editor runtime bundle.

Source: https://github.com/expo/expo

Exact local license: `node_modules/@expo/config-plugins/LICENSE`.

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## @babel/runtime

Version: 7.29.7. License: MIT.

Scope: Root devDependency and Expo transitive runtime helpers; retain when bundled by consuming app.

Source: https://github.com/babel/babel

Exact local license: `node_modules/@babel/runtime/LICENSE`.

```text
MIT License

Copyright (c) 2014-present Sebastian McKenzie and other contributors

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Example app and development scope

`example/` is a private app excluded from the root npm package. Its additional direct dependencies are listed below for maintainers who distribute the example. These notices do not imply the libraries are embedded in the editor package. A shipped example app must also include its full resolved native/JS transitive notices, beyond these direct entries.

### expo-image-picker 57.0.14 — MIT

Source: https://github.com/expo/expo.git

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### expo-image-manipulator 57.0.14 — MIT

Source: https://github.com/expo/expo.git

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### expo-status-bar 57.0.1 — MIT

Source: https://github.com/expo/expo.git

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### react-dom 19.2.3 — MIT

Source: https://github.com/facebook/react.git

```text
MIT License

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### react-native-safe-area-context 5.7.0 — MIT

Source: https://github.com/AppAndFlow/react-native-safe-area-context.git

```text
MIT License

Copyright (c) 2019 Th3rd Wave

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

The root Babel/Jest/React test renderer/testing-library/TypeScript/Prettier packages, type packages, and build/config CLI dependencies are build/test tools. Their installed transitive graph is not shipped by the editor's npm `files` list. A dependency's appearance in `devDependencies` is not proof of absence from apps: React, React Native, Expo and vector-icons are used by shipped source, and Babel runtime helpers can appear in consumers' Metro output. Attribute the actual app bundle.

## Native runtime dependency notices

These notices cover the project-owned versioned Android dependencies and their baseline transitive runtime dependencies, plus SVGgh on iOS. The baseline was resolved together with Gradle 9.3.1 using published Google Maven and Maven Central metadata on 2026-09-06, before adding the host application’s React Native/Expo dependency graph. The host can select different versions. React Native and Expo notices below identify the installed example baseline; applications must preserve notices for their actual complete resolved native/JavaScript dependency graph. Platform SDK/system libraries and development/test-only tools are not included here.

All components identified below as Apache-2.0 use the complete Apache License, Version 2.0 reproduced at the end of this native notice section. JNA is used under its Apache-2.0 option.

### AndroidX libraries (Apache-2.0)

Source: https://android.googlesource.com/platform/frameworks/support ; ConstraintLayout 2.0.1: https://github.com/androidx/constraintlayout/tree/2.0.1 . Exact Maven source archive URLs are linked per artifact.

- [androidx.activity:activity:1.8.0](https://dl.google.com/dl/android/maven2/androidx/activity/activity/1.8.0/activity-1.8.0-sources.jar)
- [androidx.annotation:annotation-experimental:1.4.0](https://dl.google.com/dl/android/maven2/androidx/annotation/annotation-experimental/1.4.0/annotation-experimental-1.4.0-sources.jar)
- [androidx.annotation:annotation-jvm:1.6.0](https://dl.google.com/dl/android/maven2/androidx/annotation/annotation-jvm/1.6.0/annotation-jvm-1.6.0-sources.jar)
- [androidx.appcompat:appcompat:1.7.0](https://dl.google.com/dl/android/maven2/androidx/appcompat/appcompat/1.7.0/appcompat-1.7.0-sources.jar)
- [androidx.appcompat:appcompat-resources:1.7.0](https://dl.google.com/dl/android/maven2/androidx/appcompat/appcompat-resources/1.7.0/appcompat-resources-1.7.0-sources.jar)
- [androidx.arch.core:core-common:2.2.0](https://dl.google.com/dl/android/maven2/androidx/arch/core/core-common/2.2.0/core-common-2.2.0-sources.jar)
- [androidx.arch.core:core-runtime:2.2.0](https://dl.google.com/dl/android/maven2/androidx/arch/core/core-runtime/2.2.0/core-runtime-2.2.0-sources.jar)
- [androidx.cardview:cardview:1.0.0](https://dl.google.com/dl/android/maven2/androidx/cardview/cardview/1.0.0/cardview-1.0.0-sources.jar)
- [androidx.collection:collection:1.1.0](https://dl.google.com/dl/android/maven2/androidx/collection/collection/1.1.0/collection-1.1.0-sources.jar)
- [androidx.concurrent:concurrent-futures:1.1.0](https://dl.google.com/dl/android/maven2/androidx/concurrent/concurrent-futures/1.1.0/concurrent-futures-1.1.0-sources.jar)
- [androidx.constraintlayout:constraintlayout:2.0.1](https://raw.githubusercontent.com/androidx/constraintlayout/2.0.1/constraintlayout/constraintlayout/src/main/java/androidx/constraintlayout/widget/ConstraintLayout.java)
- [androidx.constraintlayout:constraintlayout-solver:2.0.1](https://raw.githubusercontent.com/androidx/constraintlayout/2.0.1/constraintlayout/solver/src/main/java/androidx/constraintlayout/solver/LinearSystem.java)
- [androidx.coordinatorlayout:coordinatorlayout:1.1.0](https://dl.google.com/dl/android/maven2/androidx/coordinatorlayout/coordinatorlayout/1.1.0/coordinatorlayout-1.1.0-sources.jar)
- [androidx.core:core:1.13.0](https://dl.google.com/dl/android/maven2/androidx/core/core/1.13.0/core-1.13.0-sources.jar)
- [androidx.core:core-ktx:1.13.0](https://dl.google.com/dl/android/maven2/androidx/core/core-ktx/1.13.0/core-ktx-1.13.0-sources.jar)
- [androidx.cursoradapter:cursoradapter:1.0.0](https://dl.google.com/dl/android/maven2/androidx/cursoradapter/cursoradapter/1.0.0/cursoradapter-1.0.0-sources.jar)
- [androidx.customview:customview:1.1.0](https://dl.google.com/dl/android/maven2/androidx/customview/customview/1.1.0/customview-1.1.0-sources.jar)
- [androidx.customview:customview-poolingcontainer:1.0.0](https://dl.google.com/dl/android/maven2/androidx/customview/customview-poolingcontainer/1.0.0/customview-poolingcontainer-1.0.0-sources.jar)
- [androidx.documentfile:documentfile:1.0.0](https://dl.google.com/dl/android/maven2/androidx/documentfile/documentfile/1.0.0/documentfile-1.0.0-sources.jar)
- [androidx.drawerlayout:drawerlayout:1.1.1](https://dl.google.com/dl/android/maven2/androidx/drawerlayout/drawerlayout/1.1.1/drawerlayout-1.1.1-sources.jar)
- [androidx.dynamicanimation:dynamicanimation:1.0.0](https://dl.google.com/dl/android/maven2/androidx/dynamicanimation/dynamicanimation/1.0.0/dynamicanimation-1.0.0-sources.jar)
- [androidx.emoji2:emoji2:1.3.0](https://dl.google.com/dl/android/maven2/androidx/emoji2/emoji2/1.3.0/emoji2-1.3.0-sources.jar)
- [androidx.emoji2:emoji2-views-helper:1.3.0](https://dl.google.com/dl/android/maven2/androidx/emoji2/emoji2-views-helper/1.3.0/emoji2-views-helper-1.3.0-sources.jar)
- [androidx.fragment:fragment:1.5.4](https://dl.google.com/dl/android/maven2/androidx/fragment/fragment/1.5.4/fragment-1.5.4-sources.jar)
- [androidx.interpolator:interpolator:1.0.0](https://dl.google.com/dl/android/maven2/androidx/interpolator/interpolator/1.0.0/interpolator-1.0.0-sources.jar)
- [androidx.legacy:legacy-support-core-utils:1.0.0](https://dl.google.com/dl/android/maven2/androidx/legacy/legacy-support-core-utils/1.0.0/legacy-support-core-utils-1.0.0-sources.jar)
- [androidx.lifecycle:lifecycle-common:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-common/2.6.2/lifecycle-common-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-livedata:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-livedata/2.6.2/lifecycle-livedata-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-livedata-core:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-livedata-core/2.6.2/lifecycle-livedata-core-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-process:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-process/2.6.2/lifecycle-process-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-runtime:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-runtime/2.6.2/lifecycle-runtime-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-viewmodel:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-viewmodel/2.6.2/lifecycle-viewmodel-2.6.2-sources.jar)
- [androidx.lifecycle:lifecycle-viewmodel-savedstate:2.6.2](https://dl.google.com/dl/android/maven2/androidx/lifecycle/lifecycle-viewmodel-savedstate/2.6.2/lifecycle-viewmodel-savedstate-2.6.2-sources.jar)
- [androidx.loader:loader:1.0.0](https://dl.google.com/dl/android/maven2/androidx/loader/loader/1.0.0/loader-1.0.0-sources.jar)
- [androidx.localbroadcastmanager:localbroadcastmanager:1.0.0](https://dl.google.com/dl/android/maven2/androidx/localbroadcastmanager/localbroadcastmanager/1.0.0/localbroadcastmanager-1.0.0-sources.jar)
- [androidx.print:print:1.0.0](https://dl.google.com/dl/android/maven2/androidx/print/print/1.0.0/print-1.0.0-sources.jar)
- [androidx.profileinstaller:profileinstaller:1.3.1](https://dl.google.com/dl/android/maven2/androidx/profileinstaller/profileinstaller/1.3.1/profileinstaller-1.3.1-sources.jar)
- [androidx.recyclerview:recyclerview:1.3.2](https://dl.google.com/dl/android/maven2/androidx/recyclerview/recyclerview/1.3.2/recyclerview-1.3.2-sources.jar)
- [androidx.resourceinspection:resourceinspection-annotation:1.0.1](https://dl.google.com/dl/android/maven2/androidx/resourceinspection/resourceinspection-annotation/1.0.1/resourceinspection-annotation-1.0.1-sources.jar)
- [androidx.savedstate:savedstate:1.2.1](https://dl.google.com/dl/android/maven2/androidx/savedstate/savedstate/1.2.1/savedstate-1.2.1-sources.jar)
- [androidx.startup:startup-runtime:1.1.1](https://dl.google.com/dl/android/maven2/androidx/startup/startup-runtime/1.1.1/startup-runtime-1.1.1-sources.jar)
- [androidx.tracing:tracing:1.0.0](https://dl.google.com/dl/android/maven2/androidx/tracing/tracing/1.0.0/tracing-1.0.0-sources.jar)
- [androidx.transition:transition:1.5.0](https://dl.google.com/dl/android/maven2/androidx/transition/transition/1.5.0/transition-1.5.0-sources.jar)
- [androidx.vectordrawable:vectordrawable:1.1.0](https://dl.google.com/dl/android/maven2/androidx/vectordrawable/vectordrawable/1.1.0/vectordrawable-1.1.0-sources.jar)
- [androidx.vectordrawable:vectordrawable-animated:1.1.0](https://dl.google.com/dl/android/maven2/androidx/vectordrawable/vectordrawable-animated/1.1.0/vectordrawable-animated-1.1.0-sources.jar)
- [androidx.versionedparcelable:versionedparcelable:1.1.1](https://dl.google.com/dl/android/maven2/androidx/versionedparcelable/versionedparcelable/1.1.1/versionedparcelable-1.1.1-sources.jar)
- [androidx.viewpager2:viewpager2:1.1.0-beta02](https://dl.google.com/dl/android/maven2/androidx/viewpager2/viewpager2/1.1.0-beta02/viewpager2-1.1.0-beta02-sources.jar)
- [androidx.viewpager:viewpager:1.0.0](https://dl.google.com/dl/android/maven2/androidx/viewpager/viewpager/1.0.0/viewpager-1.0.0-sources.jar)

```text
Copyright (C) 2006 The Android Open Source Project
Copyright (C) 2009 The Android Open Source Project
Copyright (C) 2010 The Android Open Source Project
Copyright (C) 2011 The Android Open Source Project
Copyright (C) 2012 The Android Open Source Project
Copyright (C) 2013 The Android Open Source Project
Copyright (C) 2014 The Android Open Source Project
Copyright (C) 2015 Google Inc.
Copyright (C) 2015 The Android Open Source Project
Copyright (C) 2016 The Android Open Source Project
Copyright (C) 2017 The Android Open Source Project
Copyright (C) 2018 The Android Open Source Project
Copyright (C) 20188 The Android Open Source Project
Copyright (C) 2019 The Android Open Source Project
Copyright (C) 2020 The Android Open Source Project
Copyright (C) 2021 The Android Open Source Project
Copyright (C) 2022 The Android Open Source Project
Copyright (C) 2023 The Android Open Source Project
Copyright 2015 The Android Open Source Project
Copyright 2016 The Android Open Source Project
Copyright 2017 The Android Open Source Project
Copyright 2018 The Android Open Source Project
Copyright 2019 The Android Open Source Project
Copyright 2020 The Android Open Source Project
Copyright 2021 The Android Open Source Project
Copyright 2022 The Android Open Source Project
Copyright 2023 The Android Open Source Project
```

### AndroidSVG 1.4 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/com/caverock/androidsvg-aar/1.4/androidsvg-aar-1.4-sources.jar

```text
Copyright 2013 Paul LeBeau, Cave Rock Software Ltd.
Copyright 2014 Paul LeBeau, Cave Rock Software Ltd.
Copyright 2018 Paul LeBeau, Cave Rock Software Ltd.
```

### Material Components for Android 1.12.0 (Apache-2.0)

Source: https://dl.google.com/dl/android/maven2/com/google/android/material/material/1.12.0/material-1.12.0-sources.jar

```text
Copyright (C) 2014 The Android Open Source Project
Copyright (C) 2015 The Android Open Source Project
Copyright (C) 2016 The Android Open Source Project
Copyright (C) 2017 The Android Open Source Project
Copyright (C) 2018 The Android Open Source Project
Copyright (C) 2019 The Android Open Source Project
Copyright (C) 2020 The Android Open Source Project
Copyright (C) 2021 The Android Open Source Project
Copyright (C) 2022 The Android Open Source Project
Copyright (C) 2023 The Android Open Source Project
Copyright 2017 The Android Open Source Project
Copyright 2018 The Android Open Source Project
Copyright 2019 The Android Open Source Project
Copyright 2020 The Android Open Source Project
Copyright 2022 The Android Open Source Project
Copyright 2023 The Android Open Source Project
```

### Error Prone annotations 2.15.0 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/com/google/errorprone/error_prone_annotations/2.15.0/error_prone_annotations-2.15.0-sources.jar

```text
Copyright 2014 The Error Prone Authors.
Copyright 2015 The Error Prone Authors.
Copyright 2016 The Error Prone Authors.
Copyright 2017 The Error Prone Authors.
Copyright 2021 The Error Prone Authors.
```

### Guava ListenableFuture 1.0 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/com/google/guava/listenablefuture/1.0/listenablefuture-1.0-sources.jar

```text
Copyright (C) 2007 The Guava Authors
```

### OkHttp 4.9.2 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/com/squareup/okhttp3/okhttp/4.9.2/okhttp-4.9.2-sources.jar

```text
Copyright (C) 2010 The Android Open Source Project
Copyright (C) 2011 The Android Open Source Project
Copyright (C) 2012 Square, Inc.
Copyright (C) 2012 The Android Open Source Project
Copyright (C) 2013 Square, Inc.
Copyright (C) 2014 Square, Inc.
Copyright (C) 2015 Square, Inc.
Copyright (C) 2016 Square, Inc.
Copyright (C) 2017 Square, Inc.
Copyright (C) 2018 Square, Inc.
Copyright (C) 2019 Square, Inc.
Copyright (C) 2020 Square, Inc.
Copyright 2013 Twitter, Inc.
```

### Okio 2.8.0 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/com/squareup/okio/okio/2.8.0/okio-2.8.0-sources.jar

```text
Copyright (C) 2014 Square, Inc.
Copyright (C) 2015 Square, Inc.
Copyright (C) 2016 Square, Inc.
Copyright (C) 2017 Square, Inc.
Copyright (C) 2018 Square, Inc.
Copyright (C) 2019 Square, Inc.
Copyright 2014 Square Inc.
```

### JetBrains annotations 13.0 (Apache-2.0)

Source: https://repo.maven.apache.org/maven2/org/jetbrains/annotations/13.0/annotations-13.0-sources.jar

```text
Copyright 2000-2009 JetBrains s.r.o.
Copyright 2000-2012 JetBrains s.r.o.
Copyright 2000-2013 JetBrains s.r.o.
Copyright 2006 Sascha Weinreuter
```

### Kotlin standard library and coroutines (Apache-2.0, with Boost notice below)

- [org.jetbrains.kotlin:kotlin-stdlib:1.8.22](https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib/1.8.22/kotlin-stdlib-1.8.22-sources.jar)
- [org.jetbrains.kotlin:kotlin-stdlib-common:1.8.22](https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib-common/1.8.22/kotlin-stdlib-common-1.8.22-sources.jar)
- [org.jetbrains.kotlin:kotlin-stdlib-jdk7:1.6.21](https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib-jdk7/1.6.21/kotlin-stdlib-jdk7-1.6.21-sources.jar)
- [org.jetbrains.kotlin:kotlin-stdlib-jdk8:1.6.21](https://repo.maven.apache.org/maven2/org/jetbrains/kotlin/kotlin-stdlib-jdk8/1.6.21/kotlin-stdlib-jdk8-1.6.21-sources.jar)
- [org.jetbrains.kotlinx:kotlinx-coroutines-android:1.6.4](https://repo.maven.apache.org/maven2/org/jetbrains/kotlinx/kotlinx-coroutines-android/1.6.4/kotlinx-coroutines-android-1.6.4-sources.jar)
- [org.jetbrains.kotlinx:kotlinx-coroutines-core-jvm:1.6.4](https://repo.maven.apache.org/maven2/org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/1.6.4/kotlinx-coroutines-core-jvm-1.6.4-sources.jar)

```text
Copyright 2007 Google Inc.
Copyright 2010-2015 JetBrains s.r.o.
Copyright 2010-2016 JetBrains s.r.o.
Copyright 2010-2017 JetBrains s.r.o.
Copyright 2010-2018 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2010-2019 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2010-2020 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2010-2021 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2010-2022 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2010-2023 JetBrains s.r.o. and Kotlin Programming Language contributors.
Copyright 2011 The Guava Authors
Copyright 2016-2020 JetBrains s.r.o. Use of this source code is governed by the Apache 2.0 license.
Copyright 2016-2021 JetBrains s.r.o. Use of this source code is governed by the Apache 2.0 license.
Copyright 2016-2022 JetBrains s.r.o. Use of this source code is governed by the Apache 2.0 license.
Copyright Eric Ford & Hubert Holin 2001.
```

The Kotlin standard library incorporates GWT collections (Copyright 2007-08 Google Inc.) and Guava unsigned utilities (Copyright 2011 The Guava Authors), under Apache-2.0. Its inverse hyperbolic math implementations are derived from Boost special math functions.

Source: https://github.com/JetBrains/kotlin/blob/v1.8.22/license/README.md

```text
Copyright Eric Ford & Hubert Holin 2001.

Boost Software License - Version 1.0 - August 17th, 2003

Permission is hereby granted, free of charge, to any person or organization
obtaining a copy of the software and accompanying documentation covered by
this license (the "Software") to use, reproduce, display, distribute,
execute, and transmit the Software, and to prepare derivative works of the
Software, and to permit third-parties to whom the Software is furnished to
do so, all subject to the following:

The copyright notices in the Software and this entire statement, including
the above license grant, this restriction and the following disclaimer,
must be included in all copies of the Software, in whole or in part, and
all derivative works of the Software, unless such copies or derivative
works are solely in the form of machine-executable object code generated by
a source language processor.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE, TITLE AND NON-INFRINGEMENT. IN NO EVENT
SHALL THE COPYRIGHT HOLDERS OR ANYONE DISTRIBUTING THE SOFTWARE BE LIABLE
FOR ANY DAMAGES OR OTHER LIABILITY, WHETHER IN CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```

### The Public Suffix List (MPL-2.0; bundled with OkHttp 4.9.2)

Original source: https://publicsuffix.org/list/public_suffix_list.dat

OkHttp release source: https://github.com/square/okhttp/tree/parent-4.9.2/okhttp/src/main/resources/okhttp3/internal/publicsuffix

The corresponding list source used by this OkHttp release is available at https://github.com/square/okhttp/blob/parent-4.9.2/okhttp/src/test/resources/okhttp3/internal/publicsuffix/public_suffix_list.dat .

```text
Note that publicsuffixes.gz is compiled from The Public Suffix List:
https://publicsuffix.org/list/public_suffix_list.dat

It is subject to the terms of the Mozilla Public License, v. 2.0:
https://mozilla.org/MPL/2.0/
```

```text
Mozilla Public License Version 2.0
==================================

1. Definitions
--------------

1.1. "Contributor"
    means each individual or legal entity that creates, contributes to
    the creation of, or owns Covered Software.

1.2. "Contributor Version"
    means the combination of the Contributions of others (if any) used
    by a Contributor and that particular Contributor's Contribution.

1.3. "Contribution"
    means Covered Software of a particular Contributor.

1.4. "Covered Software"
    means Source Code Form to which the initial Contributor has attached
    the notice in Exhibit A, the Executable Form of such Source Code
    Form, and Modifications of such Source Code Form, in each case
    including portions thereof.

1.5. "Incompatible With Secondary Licenses"
    means

    (a) that the initial Contributor has attached the notice described
        in Exhibit B to the Covered Software; or

    (b) that the Covered Software was made available under the terms of
        version 1.1 or earlier of the License, but not also under the
        terms of a Secondary License.

1.6. "Executable Form"
    means any form of the work other than Source Code Form.

1.7. "Larger Work"
    means a work that combines Covered Software with other material, in 
    a separate file or files, that is not Covered Software.

1.8. "License"
    means this document.

1.9. "Licensable"
    means having the right to grant, to the maximum extent possible,
    whether at the time of the initial grant or subsequently, any and
    all of the rights conveyed by this License.

1.10. "Modifications"
    means any of the following:

    (a) any file in Source Code Form that results from an addition to,
        deletion from, or modification of the contents of Covered
        Software; or

    (b) any new file in Source Code Form that contains any Covered
        Software.

1.11. "Patent Claims" of a Contributor
    means any patent claim(s), including without limitation, method,
    process, and apparatus claims, in any patent Licensable by such
    Contributor that would be infringed, but for the grant of the
    License, by the making, using, selling, offering for sale, having
    made, import, or transfer of either its Contributions or its
    Contributor Version.

1.12. "Secondary License"
    means either the GNU General Public License, Version 2.0, the GNU
    Lesser General Public License, Version 2.1, the GNU Affero General
    Public License, Version 3.0, or any later versions of those
    licenses.

1.13. "Source Code Form"
    means the form of the work preferred for making modifications.

1.14. "You" (or "Your")
    means an individual or a legal entity exercising rights under this
    License. For legal entities, "You" includes any entity that
    controls, is controlled by, or is under common control with You. For
    purposes of this definition, "control" means (a) the power, direct
    or indirect, to cause the direction or management of such entity,
    whether by contract or otherwise, or (b) ownership of more than
    fifty percent (50%) of the outstanding shares or beneficial
    ownership of such entity.

2. License Grants and Conditions
--------------------------------

2.1. Grants

Each Contributor hereby grants You a world-wide, royalty-free,
non-exclusive license:

(a) under intellectual property rights (other than patent or trademark)
    Licensable by such Contributor to use, reproduce, make available,
    modify, display, perform, distribute, and otherwise exploit its
    Contributions, either on an unmodified basis, with Modifications, or
    as part of a Larger Work; and

(b) under Patent Claims of such Contributor to make, use, sell, offer
    for sale, have made, import, and otherwise transfer either its
    Contributions or its Contributor Version.

2.2. Effective Date

The licenses granted in Section 2.1 with respect to any Contribution
become effective for each Contribution on the date the Contributor first
distributes such Contribution.

2.3. Limitations on Grant Scope

The licenses granted in this Section 2 are the only rights granted under
this License. No additional rights or licenses will be implied from the
distribution or licensing of Covered Software under this License.
Notwithstanding Section 2.1(b) above, no patent license is granted by a
Contributor:

(a) for any code that a Contributor has removed from Covered Software;
    or

(b) for infringements caused by: (i) Your and any other third party's
    modifications of Covered Software, or (ii) the combination of its
    Contributions with other software (except as part of its Contributor
    Version); or

(c) under Patent Claims infringed by Covered Software in the absence of
    its Contributions.

This License does not grant any rights in the trademarks, service marks,
or logos of any Contributor (except as may be necessary to comply with
the notice requirements in Section 3.4).

2.4. Subsequent Licenses

No Contributor makes additional grants as a result of Your choice to
distribute the Covered Software under a subsequent version of this
License (see Section 10.2) or under the terms of a Secondary License (if
permitted under the terms of Section 3.3).

2.5. Representation

Each Contributor represents that the Contributor believes its
Contributions are its original creation(s) or it has sufficient rights
to grant the rights to its Contributions conveyed by this License.

2.6. Fair Use

This License is not intended to limit any rights You have under
applicable copyright doctrines of fair use, fair dealing, or other
equivalents.

2.7. Conditions

Sections 3.1, 3.2, 3.3, and 3.4 are conditions of the licenses granted
in Section 2.1.

3. Responsibilities
-------------------

3.1. Distribution of Source Form

All distribution of Covered Software in Source Code Form, including any
Modifications that You create or to which You contribute, must be under
the terms of this License. You must inform recipients that the Source
Code Form of the Covered Software is governed by the terms of this
License, and how they can obtain a copy of this License. You may not
attempt to alter or restrict the recipients' rights in the Source Code
Form.

3.2. Distribution of Executable Form

If You distribute Covered Software in Executable Form then:

(a) such Covered Software must also be made available in Source Code
    Form, as described in Section 3.1, and You must inform recipients of
    the Executable Form how they can obtain a copy of such Source Code
    Form by reasonable means in a timely manner, at a charge no more
    than the cost of distribution to the recipient; and

(b) You may distribute such Executable Form under the terms of this
    License, or sublicense it under different terms, provided that the
    license for the Executable Form does not attempt to limit or alter
    the recipients' rights in the Source Code Form under this License.

3.3. Distribution of a Larger Work

You may create and distribute a Larger Work under terms of Your choice,
provided that You also comply with the requirements of this License for
the Covered Software. If the Larger Work is a combination of Covered
Software with a work governed by one or more Secondary Licenses, and the
Covered Software is not Incompatible With Secondary Licenses, this
License permits You to additionally distribute such Covered Software
under the terms of such Secondary License(s), so that the recipient of
the Larger Work may, at their option, further distribute the Covered
Software under the terms of either this License or such Secondary
License(s).

3.4. Notices

You may not remove or alter the substance of any license notices
(including copyright notices, patent notices, disclaimers of warranty,
or limitations of liability) contained within the Source Code Form of
the Covered Software, except that You may alter any license notices to
the extent required to remedy known factual inaccuracies.

3.5. Application of Additional Terms

You may choose to offer, and to charge a fee for, warranty, support,
indemnity or liability obligations to one or more recipients of Covered
Software. However, You may do so only on Your own behalf, and not on
behalf of any Contributor. You must make it absolutely clear that any
such warranty, support, indemnity, or liability obligation is offered by
You alone, and You hereby agree to indemnify every Contributor for any
liability incurred by such Contributor as a result of warranty, support,
indemnity or liability terms You offer. You may include additional
disclaimers of warranty and limitations of liability specific to any
jurisdiction.

4. Inability to Comply Due to Statute or Regulation
---------------------------------------------------

If it is impossible for You to comply with any of the terms of this
License with respect to some or all of the Covered Software due to
statute, judicial order, or regulation then You must: (a) comply with
the terms of this License to the maximum extent possible; and (b)
describe the limitations and the code they affect. Such description must
be placed in a text file included with all distributions of the Covered
Software under this License. Except to the extent prohibited by statute
or regulation, such description must be sufficiently detailed for a
recipient of ordinary skill to be able to understand it.

5. Termination
--------------

5.1. The rights granted under this License will terminate automatically
if You fail to comply with any of its terms. However, if You become
compliant, then the rights granted under this License from a particular
Contributor are reinstated (a) provisionally, unless and until such
Contributor explicitly and finally terminates Your grants, and (b) on an
ongoing basis, if such Contributor fails to notify You of the
non-compliance by some reasonable means prior to 60 days after You have
come back into compliance. Moreover, Your grants from a particular
Contributor are reinstated on an ongoing basis if such Contributor
notifies You of the non-compliance by some reasonable means, this is the
first time You have received notice of non-compliance with this License
from such Contributor, and You become compliant prior to 30 days after
Your receipt of the notice.

5.2. If You initiate litigation against any entity by asserting a patent
infringement claim (excluding declaratory judgment actions,
counter-claims, and cross-claims) alleging that a Contributor Version
directly or indirectly infringes any patent, then the rights granted to
You by any and all Contributors for the Covered Software under Section
2.1 of this License shall terminate.

5.3. In the event of termination under Sections 5.1 or 5.2 above, all
end user license agreements (excluding distributors and resellers) which
have been validly granted by You or Your distributors under this License
prior to termination shall survive termination.

************************************************************************
*                                                                      *
*  6. Disclaimer of Warranty                                           *
*  -------------------------                                           *
*                                                                      *
*  Covered Software is provided under this License on an "as is"       *
*  basis, without warranty of any kind, either expressed, implied, or  *
*  statutory, including, without limitation, warranties that the       *
*  Covered Software is free of defects, merchantable, fit for a        *
*  particular purpose or non-infringing. The entire risk as to the     *
*  quality and performance of the Covered Software is with You.        *
*  Should any Covered Software prove defective in any respect, You     *
*  (not any Contributor) assume the cost of any necessary servicing,   *
*  repair, or correction. This disclaimer of warranty constitutes an   *
*  essential part of this License. No use of any Covered Software is   *
*  authorized under this License except under this disclaimer.         *
*                                                                      *
************************************************************************

************************************************************************
*                                                                      *
*  7. Limitation of Liability                                          *
*  --------------------------                                          *
*                                                                      *
*  Under no circumstances and under no legal theory, whether tort      *
*  (including negligence), contract, or otherwise, shall any           *
*  Contributor, or anyone who distributes Covered Software as          *
*  permitted above, be liable to You for any direct, indirect,         *
*  special, incidental, or consequential damages of any character      *
*  including, without limitation, damages for lost profits, loss of    *
*  goodwill, work stoppage, computer failure or malfunction, or any    *
*  and all other commercial damages or losses, even if such party      *
*  shall have been informed of the possibility of such damages. This   *
*  limitation of liability shall not apply to liability for death or   *
*  personal injury resulting from such party's negligence to the       *
*  extent applicable law prohibits such limitation. Some               *
*  jurisdictions do not allow the exclusion or limitation of           *
*  incidental or consequential damages, so this exclusion and          *
*  limitation may not apply to You.                                    *
*                                                                      *
************************************************************************

8. Litigation
-------------

Any litigation relating to this License may be brought only in the
courts of a jurisdiction where the defendant maintains its principal
place of business and such litigation shall be governed by laws of that
jurisdiction, without reference to its conflict-of-law provisions.
Nothing in this Section shall prevent a party's ability to bring
cross-claims or counter-claims.

9. Miscellaneous
----------------

This License represents the complete agreement concerning the subject
matter hereof. If any provision of this License is held to be
unenforceable, such provision shall be reformed only to the extent
necessary to make it enforceable. Any law or regulation which provides
that the language of a contract shall be construed against the drafter
shall not be used to construe this License against a Contributor.

10. Versions of the License
---------------------------

10.1. New Versions

Mozilla Foundation is the license steward. Except as provided in Section
10.3, no one other than the license steward has the right to modify or
publish new versions of this License. Each version will be given a
distinguishing version number.

10.2. Effect of New Versions

You may distribute the Covered Software under the terms of the version
of the License under which You originally received the Covered Software,
or under the terms of any subsequent version published by the license
steward.

10.3. Modified Versions

If you create software not governed by this License, and you want to
create a new license for such software, you may create and use a
modified version of this License if you rename the license and remove
any references to the name of the license steward (except to note that
such modified license differs from this License).

10.4. Distributing Source Code Form that is Incompatible With Secondary
Licenses

If You choose to distribute Source Code Form that is Incompatible With
Secondary Licenses under the terms of this version of the License, the
notice described in Exhibit B of this License must be attached.

Exhibit A - Source Code Form License Notice
-------------------------------------------

  This Source Code Form is subject to the terms of the Mozilla Public
  License, v. 2.0. If a copy of the MPL was not distributed with this
  file, You can obtain one at http://mozilla.org/MPL/2.0/.

If it is not possible or desirable to put the notice in a particular
file, then You may include the notice in a location (such as a LICENSE
file in a relevant directory) where a recipient would be likely to look
for such a notice.

You may add additional accurate notices of copyright ownership.

Exhibit B - "Incompatible With Secondary Licenses" Notice
---------------------------------------------------------

  This Source Code Form is "Incompatible With Secondary Licenses", as
  defined by the Mozilla Public License, v. 2.0.
```

### Java Native Access (JNA) 5.18.1 (Apache-2.0 option)

Used by both the editor and code-highlighting addon.

Source: https://github.com/java-native-access/jna/tree/5.18.1

```text
Copyright (c) 2007 Timothy Wall, All Rights Reserved
Copyright (c) 2007 Wayne Meissner, All Rights Reserved
Copyright (c) 2007-2008 Timothy Wall, All Rights Reserved
Copyright (c) 2007-2012 Timothy Wall, All Rights Reserved
Copyright (c) 2007-2013 Timothy Wall, All Rights Reserved
Copyright (c) 2007-2015 Timothy Wall, All Rights Reserved
Copyright (c) 2009 Timothy Wall, All Rights Reserved
Copyright (c) 2011 Timothy Wall, All Rights Reserved
Copyright (c) 2012 Timothy Wall, All Rights Reserved
Copyright (c) 2017 Matthias Bläsing, All Rights Reserved
Copyright (c) 2018 Matthias Bläsing
Copyright (c) 2019 Matthias Bläsing, All Rights Reserved
Copyright (c) 2021, Matthias Bläsing, All Rights Reserved
Copyright (c) 2022 Carlos Ballesteros, All Rights Reserved
Copyright (c) 1998 Sun Microsystems, Inc. All Rights Reserved.
```

```text
SPDX-License-Identifier: Apache-2.0 OR LGPL-2.1-or-later

Java Native Access (JNA) is licensed under the LGPL, version 2.1
or later, or (from version 4.0 onward) the Apache License,
version 2.0.

You can freely decide which license you want to apply to the project.

You may obtain a copy of the LGPL License at:

http://www.gnu.org/licenses/licenses.html

A copy is also included in the downloadable source code package
containing JNA, in file "LGPL2.1", under the same directory
as this file.

You may obtain a copy of the Apache License at:

http://www.apache.org/licenses/

A copy is also included in the downloadable source code package
containing JNA, in file "AL2.0", under the same directory
as this file.

Commercial support may be available, please e-mail
twall[at]users[dot]sf[dot]net.
```

### libffi bundled with JNA 5.18.1 (MIT)

Source: https://github.com/java-native-access/jna/tree/5.18.1/native/libffi

```text
libffi - Copyright (c) 1996-2022  Anthony Green, Red Hat, Inc and others.
See source files for details.

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
``Software''), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED ``AS IS'', WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

Additional copyright notices from the libffi common and Android architecture sources, subject to the MIT grant above:

```text
Copyright (C) 2007  Free Software Foundation, Inc
Copyright (C) 2007, 2009, 2010 Free Software Foundation, Inc
Copyright (C) 2008  Free Software Foundation, Inc.
Copyright (C) 2008, 2010  Free Software Foundation, Inc.
Copyright (C) 2011, 2012, 2013  Anthony Green
Copyright (C) 2021  Microsoft, Inc.
Copyright (c) 1996  Red Hat, Inc.
Copyright (c) 1996 Red Hat, Inc.
Copyright (c) 1996, 1998  Red Hat, Inc.
Copyright (c) 1996, 1998, 1999, 2001, 2007, 2008  Red Hat, Inc.
Copyright (c) 1996, 1998, 2007  Red Hat, Inc.
Copyright (c) 1996,1998,2001-2003,2005,2008,2010  Red Hat, Inc.
Copyright (c) 1996-2003  Red Hat, Inc.
Copyright (c) 1996-2003, 2007, 2008 Red Hat, Inc.
Copyright (c) 1996-2003, 2010  Red Hat, Inc.
Copyright (c) 1998, 2008, 2011  Red Hat, Inc.
Copyright (c) 1998, 2008, 2011 Red Hat, Inc.
Copyright (c) 1999, 2007, 2008  Red Hat, Inc.
Copyright (c) 1999, 2008  Red Hat, Inc.
Copyright (c) 2002  Bo Thorsen
Copyright (c) 2002  Bo Thorsen <bo@suse.de>
Copyright (c) 2002  Ranjit Mathew
Copyright (c) 2002  Roger Sayle
Copyright (c) 2002, 2007  Bo Thorsen <bo@suse.de>
Copyright (c) 2007, 2009, 2010 Red Hat, Inc.
Copyright (c) 2008  Red Hat, Inc
Copyright (c) 2008, 2010  Red Hat, Inc.
Copyright (c) 2009, 2010, 2011, 2012 ARM Ltd.
Copyright (c) 2010  CodeSourcery
Copyright (c) 2011 Anthony Green
Copyright (c) 2011 Free Software Foundation
Copyright (c) 2011 Plausible Labs Cooperative, Inc.
Copyright (c) 2011 Timothy Wall
Copyright (c) 2011, 2012, 2021  Anthony Green
Copyright (c) 2011, 2014, 2019, 2021, 2022 Anthony Green
Copyright (c) 2011, 2018, 2022  Anthony Green
Copyright (c) 2012  Anthony Green
Copyright (c) 2012, 2014, 2018  Anthony Green
Copyright (c) 2013  The Written Word, Inc.
Copyright (c) 2014  Red Hat, Inc.
Copyright (c) 2017  Anthony Green
Copyright (c) 2017, 2022  Anthony Green
Copyright (c) 2019 Microsoft Corporation.
Copyright (c) 2019, 2022 Anthony Green
Copyright (c) 2020 Madhavan T. Venkataraman
Copyright (c) 2022 Anthony Green
Copyright (c) 2022 Oracle and/or its affiliates.
```

### SVGgh 1.12.1 (MIT)

Source: https://github.com/GenerallyHelpfulSoftware/SVGgh/tree/v1.12.1

```text
The MIT License (MIT)

Copyright (c) 2014 Generally Helpful Software

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

Additional SVGgh source copyright notices, subject to the MIT grant above:

```text
Copyright (c) 2011-2014 Glenn R. Howes
Copyright (c) 2011-2015 Glenn R. Howes
Copyright (c) 2011-2018 Glenn R. Howes
Copyright (c) 2012-2014 Glenn R. Howes
Copyright (c) 2013-2014 Glenn R. Howes
Copyright (c) 2014 Glenn R. Howes
Copyright (c) 2015 Generally Helpful Software. All rights reserved.
Copyright (c) 2015 Glenn R. Howes
Copyright (c) 2016 Glenn R. Howes
Copyright © 2015 Generally Helpful. All rights reserved.
Copyright © 2016 Generally Helpful. All rights reserved.
Copyright © 2021 Generally Helpful. All rights reserved.
```

### NSData+IDZGunzip, bundled with SVGgh 1.12.1 (MIT)

Source: https://github.com/GenerallyHelpfulSoftware/SVGgh/blob/v1.12.1/SVGgh/Third%20Party/NSData%2BIDZGunzip.m

```text
Copyright (c) 2013 iOSDeveloperZone.com

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

### GzipInputStream, bundled with SVGgh 1.12.1 (Apache-2.0)

Source: https://github.com/GenerallyHelpfulSoftware/SVGgh/blob/v1.12.1/SVGgh/Third%20Party/GzipInputStream.m

```text
Copyright 2012 Eric U Kong Wong

 Licensed under the Apache License, Version 2.0 (the "License"); you may not
 use this file except in compliance with the License.  You may obtain a copy
 of the License at

 http://www.apache.org/licenses/LICENSE-2.0

 Unless required by applicable law or agreed to in writing, software
 distributed under the License is distributed on an "AS IS" BASIS, WITHOUT
 WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.  See the
 License for the specific language governing permissions and limitations under
 the License.
```

### React Native, installed host baseline 0.86.3 (MIT)

Source: https://github.com/facebook/react-native/tree/v0.86.3

This is a host-provided dependency; the library does not fix its version.

```text
MIT License

Copyright (c) Meta Platforms, Inc. and affiliates.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### ExpoModulesCore, installed host baseline 57.0.14 (MIT)

Source: https://github.com/expo/expo

This is a host-provided dependency; the library does not fix its version.

```text
The MIT License (MIT)

Copyright (c) 2015-present 650 Industries, Inc. (aka Expo)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### Apache License, Version 2.0

```text
Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
```
