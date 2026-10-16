import { createRouter, createWebHashHistory } from 'vue-router'

export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', name: 'home', component: () => import('./views/Home.vue') },
    { path: '/event/:id', name: 'event', component: () => import('./views/Home.vue') },
    { path: '/welcome', name: 'welcome', component: () => import('./views/Welcome.vue') },
    { path: '/accounts', name: 'accounts', component: () => import('./views/Accounts.vue') },
    { path: '/settings', name: 'settings', component: () => import('./views/Settings.vue') },
  ],
})
