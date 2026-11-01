using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace TimeTug.Engine;

/// <summary>
/// Implemented by an <see cref="IEngineHost"/> that can supply access tokens from the operating system's own account
/// service (Windows' Web Account Manager), so a Microsoft account needs no browser sign-in and the engine stores no
/// refresh token for it. The wrapper checks the host with <c>is IExternalTokenHost</c>: a host that implements it
/// opts in (the engine then offers the <c>microsoft-brokered</c> account kind); a host that does not behaves exactly
/// as before, and the three token requests would be answered <see cref="ServiceErrorKind.NotFound"/>.
///
/// Failures follow the same rule as every host service: throw <see cref="EngineServiceException"/> for a specific
/// <see cref="ServiceErrorKind"/> (a transport failure is <see cref="ServiceErrorKind.Network"/>); an
/// <see cref="OperationCanceledException"/> means the person dismissed the dialog; anything else is
/// <see cref="ServiceErrorKind.Failed"/>. The engine never shows the text of an exception, and the wrapper does not
/// pass it on for these requests, because it could carry account data. Return a status for the outcomes the engine
/// handles itself (<see cref="ExternalTokenStatus"/>).
/// </summary>
public interface IExternalTokenHost
{
    /// <summary>A token with no user interface. Never show a dialog from here: answer
    /// <see cref="ExternalTokenStatus.InteractionRequired"/> when the person must act. The engine gives up after
    /// 60 seconds.</summary>
    ValueTask<ExternalTokenResult> GetTokenAsync(ExternalTokenRequest request, CancellationToken cancellationToken);

    /// <summary>Shows the system's sign-in dialog. No timeout: a person is involved. The token is cancelled when the
    /// engine cancels the sign-in (and <see cref="CancelSignIn"/> is called), or when the engine is disposed.</summary>
    ValueTask<ExternalSignInResult> SignInAsync(ExternalSignInRequest request, CancellationToken cancellationToken);

    /// <summary>The person cancelled the sign-in <paramref name="signInID"/> in TimeTug: close its dialog if it is
    /// still open. Best effort; it must not block or throw.</summary>
    void CancelSignIn(string signInID);
}

/// <summary>The outcomes of a token request, as the wire spells them.</summary>
public static class ExternalTokenStatus
{
    /// <summary>The request succeeded: the result carries the token and its expiry.</summary>
    public const string Ok = "ok";

    /// <summary>The person has to sign in again (consent, password, MFA, policy). The account shows "Sign in again".</summary>
    public const string InteractionRequired = "interactionRequired";

    /// <summary>The system's account service cannot be used now or here.</summary>
    public const string Unavailable = "unavailable";

    /// <summary>Sign-in only: an administrator has to approve the app first.</summary>
    public const string Blocked = "blocked";
}

/// <summary>A silent token request for one account of the system's account service.</summary>
public sealed record ExternalTokenRequest(string ConnectionID, string AccountID, string? Email, IReadOnlyList<string> Scopes,
                                          bool ForceRefresh);

/// <summary>An interactive sign-in. <see cref="AccountID"/> is null for a first sign-in (the person picks or confirms an
/// account) and the account's id for a reconnect, with <see cref="Email"/> as a hint for the account picker.</summary>
public sealed record ExternalSignInRequest(string SignInID, IReadOnlyList<string> Scopes, string? AccountID, string? Email);

/// <summary>The answer to <see cref="IExternalTokenHost.GetTokenAsync"/>. <see cref="Reason"/> is one of the documented
/// codes (<c>consent</c>, <c>signedOut</c>, <c>mfa</c>, <c>policy</c>, <c>accountMissing</c>, <c>adminConsentRequired</c>,
/// <c>brokerUnavailable</c>, <c>other</c>), never free text from the system.</summary>
public sealed record ExternalTokenResult(string Status, string? AccessToken = null, DateTimeOffset? ExpiresAt = null,
                                         string? Reason = null)
{
    /// <summary>A token and the instant it expires.</summary>
    public static ExternalTokenResult Ok(string accessToken, DateTimeOffset expiresAt) =>
        new(ExternalTokenStatus.Ok, accessToken, expiresAt);

    /// <summary>The person must act; <paramref name="reason"/> is a documented code.</summary>
    public static ExternalTokenResult InteractionRequired(string? reason = null) =>
        new(ExternalTokenStatus.InteractionRequired, Reason: reason);

    /// <summary>The account service cannot be used; <paramref name="reason"/> is a documented code.</summary>
    public static ExternalTokenResult Unavailable(string? reason = null) =>
        new(ExternalTokenStatus.Unavailable, Reason: reason);
}

/// <summary>The answer to <see cref="IExternalTokenHost.SignInAsync"/>.</summary>
public sealed record ExternalSignInResult(string Status, string? AccountID = null, string? AccessToken = null,
                                          DateTimeOffset? ExpiresAt = null, string? Reason = null)
{
    /// <summary>The account the person chose and a token for it.</summary>
    public static ExternalSignInResult Ok(string accountID, string accessToken, DateTimeOffset expiresAt) =>
        new(ExternalTokenStatus.Ok, accountID, accessToken, expiresAt);

    /// <summary>An administrator has to approve the app; <paramref name="reason"/> is a documented code.</summary>
    public static ExternalSignInResult Blocked(string? reason = null) =>
        new(ExternalTokenStatus.Blocked, Reason: reason);

    /// <summary>The account service cannot be used; <paramref name="reason"/> is a documented code.</summary>
    public static ExternalSignInResult Unavailable(string? reason = null) =>
        new(ExternalTokenStatus.Unavailable, Reason: reason);
}
