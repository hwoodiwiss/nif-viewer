using Microsoft.AspNetCore.Components.WebAssembly.Hosting;
using NifViewer.Blazor;
using NavigationSmoke;

var builder = WebAssemblyHostBuilder.CreateDefault(args);
builder.RootComponents.Add<App>("#app");
builder.Services.AddScoped<NifViewerInterop>();
await builder.Build().RunAsync();
