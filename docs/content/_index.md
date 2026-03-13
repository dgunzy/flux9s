---
title: "flux9s"
linkTitle: "Home"
description: "A K9s-inspired terminal UI for monitoring Flux GitOps resources in real-time"
type: home
notoc: true
---

{{< rawhtml >}}

<div class="homepage-hero">
  <div class="container text-center py-5">
    <!-- ASCII Logo -->
    <div class="flux9s-logo mb-4">
<pre> _____ _             ___      
|  ___| |_   ___  __/ _ \ ___ 
| |_  | | | | \ \/ / (_) / __|
|  _| | | |_| |>  < \__, \__ \
|_|   |_|\__,_/_/\_\  /_/|___/</pre>
    </div>
    
    <!-- Tagline -->
    <p class="lead mb-5">A <a href="https://github.com/derailed/k9s" target="_blank" rel="noopener noreferrer">K9s</a>-inspired terminal UI for Flux resources, controller health, and GitOps state</p>
    
    <!-- Action Buttons -->
    <div class="d-flex flex-wrap justify-content-center gap-3 mb-5">
      <a class="btn btn-lg btn-primary" href="{{< relref \"getting-started/\" >}}">
        Get Started <i class="fas fa-arrow-alt-circle-right ms-2" aria-label="Arrow"></i><span class="icon-fallback-text">→</span>
      </a>
      <a class="btn btn-lg btn-outline-primary" href="https://github.com/dgunzy/flux9s">
        <i class="fab fa-github me-2" aria-label="GitHub"></i><span class="icon-fallback-text">[GitHub]</span><span>View on GitHub</span>
      </a>
    </div>
    
    <!-- Demo Video -->
    <div class="row justify-content-center mb-5">
      <div class="col-lg-10">
        <div class="ratio ratio-16x9" style="background: transparent;">
          <video autoplay loop muted playsinline class="w-100 h-100" style="object-fit: contain; background: transparent;" onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
            <source src="/images/demo-main.mp4" type="video/mp4">
            Your browser does not support the video tag.
          </video>
          <div style="display:none; padding: 2rem; text-align: center; background: #f8f9fa; color: #6c757d;">
            <i class="fas fa-video fa-3x mb-3"></i>
            <p class="mb-0"><strong>Demo Video</strong></p>
            <p class="small mb-2">Main interface demonstration showing navigation, resource viewing, and operations</p>
            <p class="small text-muted">Video playback is not available. The demo shows flux9s navigating resources, viewing details, and managing Flux deployments.</p>
          </div>
        </div>
        <p class="text-muted small mt-2 text-center">Watch flux9s in action - navigate resources, view details, and manage your Flux deployments</p>
      </div>
    </div>
    
    <!-- Project Stats -->
    <div class="row project-stats justify-content-center mt-5">
      <div class="col-md-3 col-sm-6 text-center stat-item mb-4">
        <h3 id="crates-downloads" class="mb-2">-</h3>
        <p class="text-muted mb-0">Crates.io Downloads</p>
      </div>
      <div class="col-md-3 col-sm-6 text-center stat-item mb-4">
        <h3 id="github-stars" class="mb-2">-</h3>
        <p class="text-muted mb-0">GitHub Stars</p>
      </div>
      <div class="col-md-3 col-sm-6 text-center stat-item mb-4">
        <h3 id="github-downloads" class="mb-2">-</h3>
        <p class="text-muted mb-0">GitHub Downloads</p>
      </div>
      <div class="col-md-3 col-sm-6 text-center stat-item mb-4">
        <h3 id="github-releases" class="mb-2">-</h3>
        <p class="text-muted mb-0">Releases</p>
      </div>
    </div>
  </div>
</div>
{{< /rawhtml >}}

{{< rawhtml >}}
<div class="container py-5">
  <div class="row justify-content-center">
    <div class="col-lg-10 col-xl-9">
      <h2>What flux9s is</h2>
      <p><code>flux9s</code> is a terminal UI for operators who want live visibility into Flux resources and the cluster state around them without leaving the shell. It watches Flux resources in real time, keeps a local in-memory view of their current state, and lets you move quickly between lists, details, YAML, traces, graphs, and reconciliation history.</p>
      <p>The project is intentionally keyboard-first and closely follows familiar <a href="https://github.com/derailed/k9s" target="_blank" rel="noopener noreferrer">K9s</a> patterns: <code>j</code>/<code>k</code> navigation, <code>:</code> command mode, context and namespace switching, footer help, and k9s-style skins.</p>

      <h2>The problem it solves</h2>
      <p>Flux already provides strong controller APIs, and the <a href="https://fluxoperator.dev/web-ui/" target="_blank" rel="noopener noreferrer">Flux Operator Web UI</a> is an excellent browser-based experience for dashboards and cluster-wide visibility. <code>flux9s</code> was built to complement that workflow, not replace it.</p>
      <p>Use <code>flux9s</code> when you want to stay in the terminal and:</p>
      <ul>
        <li>see Flux reconciliation state update live</li>
        <li>inspect Kustomizations, HelmReleases, sources, and Flux Operator resources in one place</li>
        <li>trace ownership chains and visualize managed workloads</li>
        <li>check controller readiness and Flux bundle version from the same interface</li>
        <li>run quick actions such as suspend, resume, reconcile, reconcile-with-source, and delete</li>
      </ul>

      <h2>Why it stays fast</h2>
      <p><code>flux9s</code> uses the Kubernetes Watch API for supported Flux resource types instead of repeatedly polling. By default it starts scoped to a namespace, with the default configuration targeting <code>flux-system</code>, and only switches to cluster-wide watches when you explicitly choose <code>all</code>. That keeps API usage and terminal updates lighter on larger clusters.</p>
      <p>The same watch-driven model is also used to surface Flux controller pod state and deployment metadata in the header, which is how the UI can show controller readiness and the detected Flux bundle version alongside resource health.</p>

      <h2>Built for Flux and Flux Operator</h2>
      <p>Beyond core Flux controller resources, <code>flux9s</code> also understands Flux Operator resources such as <code>FluxInstance</code>, <code>ResourceSet</code>, <code>ResourceSetInputProvider</code>, and <code>FluxReport</code>. That support shows up in practical features, not just extra rows in a table: graph and history views extend to operator-managed resources, and the graph builder follows the same relationship-discovery patterns used by the Flux Operator Web UI.</p>
      <p>For a deeper code-level walkthrough, see the <a href="{{< relref "developer-guide/" >}}">Developer Guide</a>. If you want to get running quickly, jump to <a href="{{< relref "getting-started/" >}}">Getting Started</a>.</p>
    </div>
  </div>
</div>
{{< /rawhtml >}}
