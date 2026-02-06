export const es = {
  nav: {
    home: 'Inicio',
    changelog: 'Cambios',
    licenses: 'Licencias',
    github: 'GitHub',
    download: 'Descargar',
    themeDark: 'Oscuro',
    themeOled: 'OLED',
    menu: 'Menú',
    close: 'Cerrar',
  },
  hero: {
    kicker: 'Cliente nativo de Qobuz para Linux',
    heading: 'QBZ',
    title: 'Reproducción bit-perfect, control nativo, sin límites de navegador.',
    lead: 'Qobuz transmite hasta 192 kHz. QBZ es un cliente no oficial nativo para Linux con un motor de audio en Rust que preserva el sample rate y la profundidad de bits, soporta passthrough al DAC y mantiene la reproducción transparente.',
    primaryCta: 'Descargar',
    secondaryCta: 'Ver en GitHub',
    stats: {
      audio: 'Pipeline bit-perfect',
      dac: 'Passthrough al DAC',
      native: 'Linux nativo + Rust',
    },
  },
  why: {
    title: 'Por qué existe QBZ',
    lead: 'Qobuz no ofrece una app nativa para Linux. El reproductor web depende de stacks de audio del navegador que re-muestrean, fijan tasas de salida y limitan el control del dispositivo. QBZ reemplaza el reproductor web en Linux con un pipeline nativo y control directo del output.',
    bullets: [
      'Los navegadores limitan tasas de salida y fuerzan resampling.',
      'Control limitado de dispositivos y clocks.',
      'Comportamiento inconsistente entre entornos de escritorio.',
    ],
    note: 'QBZ no reemplaza Qobuz. Reemplaza la dependencia del reproductor web en Linux.',
  },
  goals: {
    title: 'Objetivos de diseño',
    lead: 'QBZ prioriza una reproducción predecible y transparente para sesiones largas.',
    items: [
      {
        title: 'Pipeline de audio nativo',
        text: 'Sin navegador, sin resampling oculto y con manejo explícito de formatos.',
      },
      {
        title: 'Control explícito del dispositivo',
        text: 'Elige dispositivos y modos de salida sin adivinar qué hace el sistema.',
      },
      {
        title: 'Comportamiento predecible',
        text: 'Lógica visible, depurable y pensada para evitar sorpresas.',
      },
      {
        title: 'Código abierto por defecto',
        text: 'Licencia MIT, sin telemetría y desarrollo en público.',
      },
    ],
  },
  screenshots: {
    title: 'Capturas de la interfaz',
    lead: 'Vistas nativas optimizadas para sesiones largas.',
    items: [
      {
        title: 'Inicio y control de cola',
        text: 'Navegación rápida con contexto directo de reproducción.',
      },
      {
        title: 'Modo inmersivo',
        text: 'Coverflow a pantalla completa, letras y fondos ambientales.',
      },
      {
        title: 'Gestión de biblioteca local',
        text: 'Colecciones indexadas con carátulas, CUE y metadatos.',
      },
    ],
  },
  capabilities: {
    title: 'Capacidades clave',
    lead: 'Funciones puntuales para lo que el reproductor web no puede.',
    items: {
      audio: {
        title: 'Reproducción de audio nativa',
        bullets: [
          'Decodificación nativa para FLAC, ALAC, AAC y MP3.',
          'Preserva sample rate y profundidad de bits.',
          'Passthrough al DAC y modo exclusivo.',
        ],
      },
      library: {
        title: 'Biblioteca local',
        bullets: [
          'Escaneo de carpetas con extracción de metadatos.',
          'Descubrimiento y caché de carátulas.',
          'Soporte CUE e indexado en SQLite.',
        ],
      },
      playlists: {
        title: 'Interoperabilidad de playlists',
        bullets: [
          'Importa desde Spotify, Apple Music, Tidal y Deezer.',
          'Matching local con preferencia por calidad.',
          'Sin servicios externos de conversión.',
        ],
      },
      desktop: {
        title: 'Integración con Linux',
        bullets: [
          'MPRIS y teclas multimedia.',
          'Notificaciones y atajos de teclado.',
          'Enumeración y selección PipeWire.',
        ],
      },
      casting: {
        title: 'Reproducción en red',
        bullets: [
          'Soporte Chromecast y DLNA/UPnP.',
          'Selector unificado con handoff.',
          'Keepalive estable para dispositivos.',
        ],
      },
      radio: {
        title: 'Radio',
        bullets: [
          'Playlists de radio locales y deterministas.',
          'Experiencia de escucha consistente.',
          'Transparente y explicable.',
        ],
      },
      offline: {
        title: 'Modo offline',
        bullets: [
          'Funciona sin internet—o por elección.',
          'Accede a tu biblioteca local sin conexión.',
          'Escucha ahora, sincroniza después.',
        ],
      },
      immersive: {
        title: 'Reproductor Inmersivo',
        bullets: [
          'Fondos ambientales impulsados por WebGL.',
          'Letras, coverflow y paneles enfocados.',
          'Experiencia visual libre de distracciones.',
        ],
      },
      dacWizard: {
        title: 'Asistente DAC',
        bullets: [
          'Configuración guiada para PipeWire bit-perfect.',
          'Genera comandos específicos por distro.',
          'Simplifica la configuración de audio compleja.',
        ],
      },

      discovery: {
        title: 'Descubrimiento Inteligente',
        bullets: [
          'Sugerencias basadas en vectores.',
          'Motor de similitud local para joyas ocultas.',
          'Encuentra tracks manteniendo el vibe.',
        ],
      },
      genres: {
        title: 'Filtrado Avanzado',
        bullets: [
          'Jerarquía de géneros de tres niveles.',
          'Precisión de subgéneros por contexto.',
          'Explora más allá de las categorías básicas.',
        ],
      },
      metadata: {
        title: 'Metadatos y créditos',
        bullets: [
          'Integración con MusicBrainz para enriquecer artistas y álbumes.',
          'Páginas de músicos con roles, créditos y discografía.',
          'Editor de tags para biblioteca local con almacenamiento sidecar no destructivo.',
        ],
      },
      hideArtists: {
        title: 'Ocultar Artistas',
        bullets: [
          'Bloquea artistas de tu biblioteca y recomendaciones.',
          'Limpia los feeds de descubrimiento automáticamente.',
          'Persistente entre sesiones.',
        ],
      },
      songRecommendations: {
        title: 'Recomendaciones de Canciones',
        bullets: [
          'Sugerencias algorítmicas basadas en tu historial local de reproducción.',
          'Powered by combinación única de metadatos de Qobuz y MusicBrainz.',
          'Expande playlists con un clic.',
        ],
      },
    },
  },
  downloads: {
    title: 'Descargas',
    lead: 'Las builds se obtienen desde GitHub Releases. Elige el formato ideal para tu distro.',
    recommendedLabel: 'Recomendado para tu sistema',
    allLabel: 'Todas las descargas disponibles',
    loading: 'Cargando datos de la release…',
    error: 'No se pudo cargar la release. Usa la página de GitHub Releases.',
    versionLabel: 'Release',
    viewAll: 'Ver todas las releases',
    fileCount: '{{count}} archivos',
    instructionsTitle: 'Comandos de instalación',
    instructions: {
      aur: 'yay -S qbz-bin',
      appimage: 'chmod +x QBZ.AppImage && ./QBZ.AppImage',
      deb: 'sudo dpkg -i qbz_*.deb',
      rpm: 'sudo rpm -i qbz-*.rpm',
      flatpak: 'flatpak install --user ./qbz.flatpak',
      tarball: 'tar -xzf qbz.tar.gz && ./qbz',
    },
    buildTitle: 'Compilar desde el código (avanzado)',
    buildBody: 'QBZ está enfocado en Linux. En macOS puede compilar, pero funciones como PipeWire, casting y control de dispositivos pueden estar incompletas o inestables.',
    buildInstructions: {
      summary: 'Mostrar instrucciones de compilación',
      prereqTitle: 'Requisitos previos',
      nodeNote: 'Se requiere Node.js 20+. Usa nvm, fnm o el gestor de paquetes de tu distro.',
      cloneTitle: 'Clonar y compilar',
      apiTitle: 'API keys (opcional)',
      apiLead: 'Las API keys se integran en tiempo de compilación. Copia el archivo de ejemplo y agrega tus keys:',
      apiBody: 'Edita .env con tus API keys, luego ejecuta npm run dev:tauri para cargarlas automáticamente.',
      apiKeysTitle: 'Dónde obtener API keys',
      apiOptional: 'Todas las integraciones son opcionales. La app funciona sin ellas, pero las funciones correspondientes estarán deshabilitadas.',
    },
    buildDisclaimer: 'Si generas tus propios binarios, tú administras las API keys y dependencias de plataforma.',
  },
  audience: {
    title: 'Para quién es',
    lead: 'QBZ está pensado para quien quiere una ruta de reproducción nativa y transparente en Linux.',
    items: [
      'Usuarios Linux que buscan un cliente real de Qobuz.',
      'Audiófilos que cuidan sample rate, bit depth y DAC.',
      'Quien prefiere herramientas nativas sobre wrappers.',
      'Usuarios que quieren streaming y biblioteca local en un solo lugar.',
    ],
    notFor: 'QBZ no intenta reemplazar a Qobuz como servicio.',
  },
  openSource: {
    title: 'Código abierto y transparente',
    lead: 'QBZ es FOSS, sin telemetría ni tracking.',
    items: [
      'Licencia MIT y desarrollo público.',
      'Sin analíticas, anuncios ni tracking en segundo plano.',
      'Integraciones opcionales solo si tú las habilitas.',
      'Inspirado por el ecosistema FOSS de audio y la comunidad de audio de Linux.',
    ],
  },
  linuxFirst: {
    title: 'Linux first',
    lead: 'QBZ se desarrolla y prueba en Linux. Las builds para macOS son experimentales y pueden carecer de funciones o estabilidad.',
  },
  apis: {
    title: 'API keys opcionales',
    lead: 'Las API keys solo son necesarias si compilas QBZ por tu cuenta. Las releases incluyen lo necesario para funciones estándar.',
    summary: 'Mostrar integraciones opcionales',
    items: [
      'Scrobbling y now-playing de Last.fm.',
      'Búsqueda de carátulas en Discogs.',
      'Importación de playlists de Spotify y Tidal.',
      'Compartir con Song.link.',
    ],
  },
  footer: {
    disclaimer: 'Esta aplicación usa la API de Qobuz pero no está certificada, afiliada ni respaldada por Qobuz.',
    rights: 'Publicado bajo licencia MIT.',
  },
  changelog: {
    title: 'Historial de cambios',
    lead: 'Las notas de versión se cargan directamente desde GitHub Releases.',
    latestLabel: 'Última release',
    loading: 'Cargando notas de versión…',
    empty: 'Aún no hay releases publicadas.',
    viewOnGitHub: 'Ver notas completas en GitHub',
  },
  licenses: {
    title: 'Licencias y atribuciones',
    lead: 'QBZ usa licencia MIT y se apoya en librerías y APIs abiertas.',
    qbzLicense: 'Licencia de QBZ',
    qbzLicenseBody: 'QBZ se publica bajo la licencia MIT.',
    viewLicense: 'Ver licencia en GitHub',
    categories: {
      core: {
        title: 'Tecnologías base',
        items: ['Rust', 'Tauri', 'Svelte', 'Vite', 'SQLite'],
      },
      audio: {
        title: 'Librerías de audio y media',
        items: ['Rodio', 'Symphonia', 'Lofty'],
      },
      casting: {
        title: 'Casting y networking',
        items: ['rust_cast', 'DLNA/UPnP AVTransport'],
      },
      lyrics: {
        title: 'Proveedores de letras',
        items: ['LRCLIB', 'lyrics.ovh'],
      },
      integrations: {
        title: 'Integraciones y APIs',
        items: ['Qobuz', 'Last.fm API', 'Discogs API', 'Spotify API', 'Tidal API', 'Song.link (Odesli)'],
      },
      inspiration: {
        title: 'Inspiración',
        items: ['Comunidad de audio de Linux', 'Ecosistema FOSS de audio'],
      },
      website: {
        title: 'Stack del sitio',
        items: ['React', 'Vite', 'TypeScript', 'i18next', 'react-i18next'],
      },
    },
    acknowledgments: 'Gracias a los proyectos open source y proveedores de APIs que hacen posible QBZ.',
    qobuzDisclaimer: 'Qobuz es una marca registrada de su respectivo propietario. QBZ no está afiliado a Qobuz.',
  },
  comingSoon: {
    title: 'Próximamente / Experimental',
    lead: 'Funciones actualmente en desarrollo o pruebas.',
    badge: 'Experimental',
    items: [
      {
        title: 'API de Control Remoto',
        text: 'Operación headless y soporte para control externo.',
      },
      {
        title: 'Visualizador de Audio Avanzado',
        text: 'Analizador de espectro y visualización de forma de onda.',
      },
    ],
  },
  about: {
    title: '¿Por qué QBZ?',
    content: `QBZ es un proyecto personal que vio la luz hace poco más de {{years}} años. Comenzó cuando usé el código de qobuz-dl para crear un backend API local que me permitiera buscar música y escucharla en mi equipo. Meses —quizás un año— después, ante el hype de migrar todo a Rust y como experimento para aprender un lenguaje nuevo y agregarlo a mi stack tecnológico, migré dicho backend a Rust. También hice una interfaz web bastante artesanal que al menos me permitía obtener mis playlists de Qobuz y usarlo como media player sin distracciones. Aún confiaba en que pronto habría un cliente oficial. Francamente, con todo y que me declaro entusiasta de Linux, no soy fan de los music players en terminal —uso tanto la terminal que a veces la cierro sin más, y eso causa que me quede sin música por cerrar la ventana equivocada.

Como mucha gente en 2025, integré el uso de agentes de código en mi flujo de trabajo (el real, el que paga las facturas). Esto me hizo pensar en desbloquear este proyecto de mi stack personal. Tomé ideas de los reproductores de música que uso normalmente, features que creo que a gente como yo le gustarían y —sí, si se lo preguntan, "¿Esta app está vibecodeada?"— la respuesta es sí, sin vergüenza. Pero cabe aclarar: soy ingeniero de software, así que he procurado incorporar las mejores prácticas, estructuras de diseño y arquitectura adecuada. Solo la planeación, escritura de prompts, plan de arquitectura y orquestación me tomó un par de semanas. Este proyecto no es un "Hice un nuevo ERP en 3 días sin escribir una sola línea de código". Cada bloque de código ha sido revisado como si se tratara de revisar el código de un becario. No creo en el zero-code, pero tampoco odio el vibecoding. Creo en adaptarse o morir, y que toda herramienta es útil si se usa con responsabilidad. Si tienen curiosidad de qué herramientas fueron usadas: Claude Code, GPT Codex, Copilot y Figma AI me han tenido que tolerar a mí y a mis cambios de humor y de decisiones —se las recomiendo.`,
    donationsTitle: 'Donativos',
    donationsContent: `Si deseas apoyar a QBZ, te lo agradezco sinceramente. Dicho esto, hay proyectos que han sido clave en mi flujo de trabajo y merecen reconocimiento: KDE Plasma, Neovim y por supuesto Arch Linux (I use Arch BTW). Considera dividir tu generosidad—o donar a ellos en nombre de QBZ. De cualquier forma, tu feedback y buenos comentarios ya significan mucho. Ojos frescos siempre son lo mejor para el QA de un desarrollador en solitario. Claro, un café no se puede rechazar.`,
    donationLinks: {
      kde: 'KDE Plasma',
      neovim: 'Neovim',
      arch: 'Arch Linux',
    },
  },

  qobuzLinux: {
    hero: {
      kicker: 'Cliente nativo de Qobuz para Linux',
      title: 'Qobuz para Linux — Reproductor Hi-Fi Nativo (No es un Web Wrapper)',
      lead1: 'QBZ es un cliente de escritorio nativo para Qobuz™, construido para usuarios que buscan reproducción bit-perfect, control directo del DAC y audio de alta resolución real.',
      lead2: 'A diferencia de los reproductores web o web wrappers, QBZ no depende de Chromium ni WebAudio. Utiliza una tubería de audio nativa diseñada específicamente para Linux.',
      ctaDownload: 'Descargar QBZ',
      ctaGithub: 'Ver en GitHub',
    },
    whyNative: {
      title: 'Por qué Qobuz necesita un cliente nativo',
      lead: 'Qobuz transmite audio sin pérdidas hasta 24-bit/192 kHz. Pero sin una aplicación nativa en Linux, los usuarios están forzados a usar el reproductor web o wrappers de terceros—ambos comprometen la calidad del audio.',
      bullets: [
        'El reproductor web oficial usa el stack de audio del navegador que remuestrea a 48 kHz.',
        'Los wrappers web (basados en Electron) heredan las mismas limitaciones de WebAudio.',
        'Los audiófilos en Linux no tienen forma de lograr reproducción bit-perfect a través de un navegador.',
        'El passthrough al DAC y el modo exclusivo son imposibles vía WebAudio.',
      ],
    },
    different: {
      title: 'Qué hace diferente a QBZ',
      lead: 'QBZ no es un web wrapper. Es una aplicación nativa construida con Rust y Tauri, usando un motor de audio dedicado que evita totalmente las limitaciones del navegador.',
      features: [
        { title: 'Tubería de audio nativa', text: 'Decodificadores integrados para FLAC, ALAC, AAC y MP3. Sin stack de audio del navegador. Sin remuestreo oculto.' },
        { title: 'Acceso directo al DAC', text: 'Soporta modo exclusivo ALSA (hw: devices) y passthrough de PipeWire para salida bit-perfect.' },
        { title: 'Cambio de frecuencia por pista', text: 'Ajusta automáticamente la frecuencia de salida para coincidir con la fuente (44.1, 48, 88.2, 96, 176.4, 192 kHz).' },
        { title: 'Sin Chromium', text: 'QBZ usa Tauri (UI basada en WebView) con un backend en Rust. No empaqueta Chromium ni Electron.' },
      ],
    },
    bitPerfect: {
      title: 'Reproducción Bit-perfect en Linux',
      lead: 'QBZ soporta dos configuraciones principales de backend de audio para lograr reproducción bit-perfect.',
      alsa: {
        title: 'ALSA Directo (hw: devices)',
        text: 'Para máximo control, QBZ puede enviar audio directamente a dispositivos de hardware ALSA, saltándose PulseAudio y PipeWire por completo. Esto habilita el modo exclusivo, donde QBZ toma control total del DAC.',
        bullets: [
          'Acceso exclusivo al dispositivo de audio (sin mezcla con sonidos del sistema).',
          'Salida bit-perfect real—sin remuestreo, sin conversión de formato.',
          'Cambio de frecuencia de muestreo por pista a nivel de hardware.',
        ],
      },
      pipewire: {
        title: 'PipeWire (configuración avanzada)',
        text: 'Para usuarios de PipeWire, QBZ puede configurarse en modo passthrough con reglas de WirePlumber, logrando una salida casi bit-perfect manteniendo la integración del sistema.',
        bullets: [
          'Compatible con escritorios modernos de Linux (Fedora, Arch, etc.).',
          'Soporta delegación de control de volumen por hardware al DAC.',
          'QBZ incluye un asistente de configuración de DAC para generar la configuración necesaria.',
        ],
      },
    },
    wrappers: {
      title: 'Por qué los web wrappers se quedan cortos',
      lead: 'Los web wrappers empaquetan el reproductor web de Qobuz en una carcasa de navegador. Parecen apps nativas, pero heredan todas las limitaciones de audio de los navegadores.',
      bullets: [
        'La API WebAudio remuestrea todo el audio a 48 kHz, sin importar la calidad de la fuente.',
        'Sin acceso a ALSA o PipeWire—el audio pasa por el stack del navegador.',
        'No pueden solicitar modo exclusivo o passthrough al DAC.',
        'El contenido Hi-Res (88.2, 96, 176.4, 192 kHz) es submuestreado antes de reproducirse.',
        'Sin cambio de frecuencia de muestreo por pista.',
      ],
      note: 'Si usas un web wrapper y esperas audio Hi-Res, probablemente estés escuchando una salida remuestreada a 48 kHz.',
    },
    comparison: {
      title: 'QBZ vs reproductores web',
      lead: 'Comparación técnica de capacidades de audio.',
      headers: ['Característica', 'QBZ', 'Web Player / Wrappers'],
      rows: [
        { feature: 'Tubería de audio nativa', qbz: true, web: false, webText: '✗' },
        { feature: 'Reproducción Bit-perfect', qbz: true, web: false, webText: '✗' },
        { feature: 'Modo exclusivo ALSA', qbz: true, web: false, webText: '✗' },
        { feature: 'Passthrough al DAC', qbz: true, web: false, webText: '✗' },
        { feature: 'Cambio de frecuencia por pista', qbz: true, web: false, webText: '✗' },
        { feature: 'Salida Hi-Res (88.2–192 kHz)', qbz: true, web: false, webText: 'Remuestreado a 48 kHz' },
        { feature: 'Sin Chromium/Electron', qbz: true, web: false, webText: '✗' },
      ],
    },
    features: {
      title: 'Características de un vistazo',
      items: [
        { title: 'Streaming de Qobuz', text: 'Acceso total a tu librería, favoritos y playlists de Qobuz.' },
        { title: 'Librería local', text: 'Indexa y reproduce archivos locales FLAC/ALAC/MP3 junto al contenido de Qobuz.' },
        { title: 'Chromecast y DLNA', text: 'Envía audio a dispositivos de red con manejo estable de reproducción.' },
        { title: 'Integración MPRIS', text: 'Las teclas multimedia y controles de escritorio funcionan de inmediato.' },
        { title: 'Letras y metadatos', text: 'Enriquecimiento con MusicBrainz, créditos y letras sincronizadas.' },
        { title: 'Importación de Playlists', text: 'Importa playlists de Spotify, Apple Music, Tidal y Deezer.' },
      ],
    },
    forWho: {
      title: 'Para quién es QBZ',
      bullets: [
        'Usuarios de Linux que quieren un cliente de escritorio nativo de Qobuz.',
        'Audiófilos que se preocupan por la frecuencia de muestreo, profundidad de bits y control del DAC.',
        'Usuarios frustrados por las limitaciones de audio de los navegadores.',
        'Cualquiera que quiera streaming y librería local en una sola aplicación.',
      ],
      note: 'QBZ no es un reemplazo de Qobuz. Es una interfaz nativa para usuarios que quieren más control sobre su reproducción de audio en Linux.',
    },
    openSource: {
      title: 'Open source y transparente',
      bullets: [
        'Licencia MIT—gratis para usar, modificar y distribuir.',
        'Sin telemetría, sin analíticas, sin rastreo.',
        'Código fuente disponible en GitHub.',
        'Desarrollado en abierto con seguimiento público de problemas.',
      ],
    },
    install: {
      title: 'Instalación',
      lead: 'QBZ está disponible como AppImage, .deb, .rpm, Flatpak y paquetes AUR.',
      cta: 'Ver todas las descargas',
    },
    legal: {
      title: 'Aviso legal',
      text: 'Qobuz es una marca registrada de Xandrie SA. QBZ es un proyecto independiente y no oficial. No está certificado, afiliado ni respaldado por Qobuz. QBZ utiliza la API de Qobuz de acuerdo con sus términos de servicio. Se requiere una suscripción válida a Qobuz para usar QBZ.',
    },
  },
}
