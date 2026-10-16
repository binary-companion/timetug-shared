using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace TimeTug.Engine;

/// <summary>
/// The services the engine asks its host for (the <c>service.*</c> requests of the contract). The wrapper calls these
/// on the engine's delivery thread, or through <see cref="EngineOptions.Dispatcher"/> when one is given, and sends
/// the answer back with <c>tt_engine_reply</c>. A method may complete synchronously or later; requests may be answered
/// in any order. To fail a request, throw <see cref="EngineServiceException"/> with the kind the engine should see
/// (any other exception is reported as <see cref="ServiceErrorKind.Failed"/>, an
/// <see cref="OperationCanceledException"/> as <see cref="ServiceErrorKind.Cancelled"/>). The token is cancelled when
/// the engine is disposed.
/// </summary>
public interface IEngineHost
{
    /// <summary>The secure store for connection secrets (Credential Locker on Windows).</summary>
    ICredentialStore Credentials { get; }

    /// <summary>The HTTP client the connectors use.</summary>
    IHttpTransport Http { get; }

    /// <summary>Browser and form sign-in.</summary>
    IAuthorizationInteraction Authorization { get; }

    /// <summary>On-device text generation, or null when the host has none (the engine then does not ask).</summary>
    ITextGenerator? TextGenerator { get; }

    /// <summary>Every diagnostic: the engine's (also delivered as <see cref="EngineEvent.Diagnostic"/>) and the
    /// wrapper's own (category <c>wrapper</c>). For logging; must not block.</summary>
    void Diagnostic(EngineDiagnostic diagnostic);
}

/// <summary>Connection secrets, keyed by connection id.</summary>
public interface ICredentialStore
{
    /// <summary>The secrets of a connection, or null when it has none.</summary>
    ValueTask<IReadOnlyDictionary<string, string>?> GetAsync(string connectionID, CancellationToken cancellationToken);

    /// <summary>Replaces the secrets of a connection.</summary>
    ValueTask SetAsync(string connectionID, IReadOnlyDictionary<string, string> secrets, CancellationToken cancellationToken);

    /// <summary>Removes a connection's secrets (no error when there are none).</summary>
    ValueTask RemoveAsync(string connectionID, CancellationToken cancellationToken);

    /// <summary>The secrets with an opaque revision string, or null when the connection has none.</summary>
    ValueTask<CredentialSnapshot?> SnapshotAsync(string connectionID, CancellationToken cancellationToken);

    /// <summary>Atomically stores a new refresh token (key <c>refresh_token</c>) when the stored revision still equals
    /// <paramref name="expectedRevision"/>; returns false, writing nothing, when it does not.</summary>
    ValueTask<bool> UpdateRefreshTokenAsync(string connectionID, string token, string expectedRevision,
                                            CancellationToken cancellationToken);
}

/// <summary>Sends HTTP requests for the engine. 4xx and 5xx are results, not errors; a transport failure is an
/// <see cref="EngineServiceException"/> of kind <see cref="ServiceErrorKind.Network"/>. The engine times a request out
/// after 60 seconds on its own.</summary>
public interface IHttpTransport
{
    /// <summary>Sends one request.</summary>
    ValueTask<EngineHttpResponse> SendAsync(EngineHttpRequest request, CancellationToken cancellationToken);
}

/// <summary>Sign-in: the browser (OAuth redirect) flow and the in-app credential form.</summary>
public interface IAuthorizationInteraction
{
    /// <summary>Starts listening for an OAuth redirect; returns the session id and the redirect URI to register.</summary>
    ValueTask<RedirectSession> BeginRedirectAsync(CancellationToken cancellationToken);

    /// <summary>Opens <paramref name="url"/> in the browser and returns the redirect URL the provider sent back. No
    /// timeout: a person is involved.</summary>
    ValueTask<string> AuthorizeAsync(string sessionID, string url, CancellationToken cancellationToken);

    /// <summary>Stops listening for the session's redirect.</summary>
    ValueTask CloseRedirectAsync(string sessionID, CancellationToken cancellationToken);

    /// <summary>Asks the person for the given fields; returns the values by field key.</summary>
    ValueTask<IReadOnlyDictionary<string, string>> PromptCredentialsAsync(IReadOnlyList<CredentialField> fields,
                                                                          CancellationToken cancellationToken);

    /// <summary>Prepares the sign-in form (title, help, prefilled values, the previous attempt's error).</summary>
    ValueTask PrepareSignInAsync(SignInForm form, CancellationToken cancellationToken);

    /// <summary>The non-secret values of the last submitted form, or null when none was submitted.</summary>
    ValueTask<IReadOnlyDictionary<string, string>?> LastNonSecretValuesAsync(CancellationToken cancellationToken);
}

/// <summary>On-device text generation, used by the engine's duplicate detection.</summary>
public interface ITextGenerator
{
    /// <summary>Whether generation is available now.</summary>
    ValueTask<TextAvailability> AvailabilityAsync(CancellationToken cancellationToken);

    /// <summary>Generates text for a prompt.</summary>
    ValueTask<string> GenerateAsync(string instructions, string prompt, CancellationToken cancellationToken);
}

/// <summary>A connection's secrets and their revision (an opaque string the engine passes back unchanged).</summary>
public sealed record CredentialSnapshot(IReadOnlyDictionary<string, string> Secrets, string Revision);

/// <summary>An HTTP request from the engine.</summary>
public sealed record EngineHttpRequest(string Method, string Url, IReadOnlyDictionary<string, string> Headers, byte[]? Body);

/// <summary>An HTTP response for the engine.</summary>
public sealed record EngineHttpResponse(int Status, IReadOnlyDictionary<string, string>? Headers = null, byte[]? Body = null);

/// <summary>A started OAuth redirect listener.</summary>
public sealed record RedirectSession(string SessionID, string RedirectURI);

/// <summary>One field of a credential form. <see cref="Label"/> comes from the connector library in English; a
/// localizing front end keys its own labels on <see cref="Key"/>.</summary>
public sealed record CredentialField(string Key, string Label, bool IsSecret);

/// <summary>Help shown with a credential form.</summary>
public sealed record CredentialHelp(string Text, string? LinkTitle = null, string? Url = null);

/// <summary>The sign-in form to prepare.</summary>
public sealed record SignInForm(string Title, CredentialHelp? Help, IReadOnlyDictionary<string, string> Values, string? Error);

/// <summary>Text generation availability. <see cref="Kind"/>: <c>available</c>, <c>unavailable</c> or
/// <c>temporarilyUnavailable</c>.</summary>
public sealed record TextAvailability(string Kind, string? Reason = null, string? EngineID = null, string? EngineName = null,
                                      bool? IsOnDevice = null);

/// <summary>The error kinds of a service reply.</summary>
public enum ServiceErrorKind
{
    /// <summary>A transport failure (the HTTP request did not complete).</summary>
    Network,
    /// <summary>The operation was cancelled (by the person or by disposal).</summary>
    Cancelled,
    /// <summary>The item does not exist, or the host does not offer the service.</summary>
    NotFound,
    /// <summary>The person or the system refused.</summary>
    Denied,
    /// <summary>Anything else.</summary>
    Failed,
}

/// <summary>Thrown by a host service to fail a request with a specific <see cref="ServiceErrorKind"/>.</summary>
public sealed class EngineServiceException : Exception
{
    /// <summary>Creates the exception.</summary>
    public EngineServiceException(ServiceErrorKind kind, string message, Exception? innerException = null)
        : base(message, innerException) => Kind = kind;

    /// <summary>The kind the engine receives.</summary>
    public ServiceErrorKind Kind { get; }
}
