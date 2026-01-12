import '../scss/main.scss'

// Header scroll effect
const header = document.querySelector('.header')
if (header) {
  const updateHeader = () => {
    if (window.scrollY > 50) {
      header.classList.add('scrolled')
    } else {
      header.classList.remove('scrolled')
    }
  }

  window.addEventListener('scroll', updateHeader, { passive: true })
  updateHeader()
}

// Smooth scroll for anchor links
document.querySelectorAll('a[href^="#"]').forEach(anchor => {
  anchor.addEventListener('click', function (e) {
    const href = this.getAttribute('href')
    if (href === '#') return

    const target = document.querySelector(href)
    if (target) {
      e.preventDefault()
      target.scrollIntoView({
        behavior: 'smooth',
        block: 'start'
      })
    }
  })
})

// Intersection Observer for fade-in animations
const observerOptions = {
  threshold: 0.1,
  rootMargin: '0px 0px -50px 0px'
}

const fadeInObserver = new IntersectionObserver((entries) => {
  entries.forEach(entry => {
    if (entry.isIntersecting) {
      entry.target.classList.add('visible')
      fadeInObserver.unobserve(entry.target)
    }
  })
}, observerOptions)

// Add fade-in animation to elements
document.querySelectorAll('.feature-card, .showcase__content, .license-card, .changelog-item, .stat').forEach(el => {
  el.classList.add('fade-in')
  fadeInObserver.observe(el)
})

// Stagger animation for grid items
document.querySelectorAll('.features-grid, .licenses-grid, .stats__grid').forEach(grid => {
  const items = grid.children
  Array.from(items).forEach((item, index) => {
    item.style.transitionDelay = `${index * 0.1}s`
  })
})
