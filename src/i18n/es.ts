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
      casting: 'Chromecast + DLNA',
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
        title: 'Modo focus',
        text: 'Escucha en pantalla completa con letras y contexto del dispositivo.',
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
      'Inspirado por el ecosistema FOSS de Qobuz, incluyendo qobuz-dl.',
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
    disclaimer: 'QBZ es un proyecto independiente y no está afiliado a Qobuz.',
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
        items: ['LRCLIB'],
      },
      integrations: {
        title: 'Integraciones y APIs',
        items: ['Qobuz', 'Last.fm API', 'Discogs API', 'Spotify API', 'Tidal API', 'Song.link (Odesli)'],
      },
      website: {
        title: 'Stack del sitio',
        items: ['React', 'Vite', 'TypeScript', 'i18next', 'react-i18next'],
      },
    },
    acknowledgments: 'Gracias a los proyectos open source y proveedores de APIs que hacen posible QBZ.',
    qobuzDisclaimer: 'Qobuz es una marca registrada de su respectivo propietario. QBZ no está afiliado a Qobuz.',
  },
}
