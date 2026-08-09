namespace NifViewer.Blazor;

/// <summary>Resolves dependencies by GET-ing <c>{baseUrl}/{path}</c>; 404 (or any failure) yields null.</summary>
public sealed class HttpNifDependencyResolver(HttpClient httpClient, string baseUrl) : INifDependencyResolver
{
    private readonly string _baseUrl = baseUrl.TrimEnd('/');

    public async Task<byte[]?> ResolveAsync(string path)
    {
        try
        {
            using var response = await httpClient.GetAsync($"{_baseUrl}/{path.TrimStart('/')}");
            if (!response.IsSuccessStatusCode)
            {
                return null;
            }

            return await response.Content.ReadAsByteArrayAsync();
        }
        catch (HttpRequestException)
        {
            return null;
        }
    }
}
