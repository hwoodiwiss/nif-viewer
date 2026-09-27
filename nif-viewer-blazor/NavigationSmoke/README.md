# Packaged navigation smoke host

Pack `NifViewer.Blazor` into `artifacts` first, then:

```powershell
dotnet restore nif-viewer-blazor/NavigationSmoke
dotnet run --project nif-viewer-blazor/NavigationSmoke --urls http://localhost:5198
```

This consumes the local NuGet package, not a project reference. Test keyboard and
controller activation, settings, typing in the host field, and repeated attach /
detach while moving. The renderer rehomes its live canvas on each new component.
Use the standalone generated-scene browser suite to validate rendered geometry.

After building this host, set `BROWSER_EXECUTABLE` and run `npm run test:blazor`
from `nif-viewer-webapp` for automated package lifecycle/settings verification.
The script starts and stops its own local smoke server.
