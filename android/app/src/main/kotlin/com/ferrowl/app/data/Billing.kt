package com.ferrowl.app.data

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import androidx.core.content.edit
import androidx.core.net.toUri
import com.android.billingclient.api.AcknowledgePurchaseParams
import com.android.billingclient.api.BillingClient
import com.android.billingclient.api.BillingClientStateListener
import com.android.billingclient.api.BillingFlowParams
import com.android.billingclient.api.BillingResult
import com.android.billingclient.api.PendingPurchasesParams
import com.android.billingclient.api.ProductDetails
import com.android.billingclient.api.Purchase
import com.android.billingclient.api.PurchasesUpdatedListener
import com.android.billingclient.api.QueryProductDetailsParams
import com.android.billingclient.api.QueryPurchasesParams
import com.ferrowl.app.BuildConfig
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Ferrowl Pro: a yearly subscription sold through Google Play Billing
 * (product [PRODUCT], created in Play Console). Pro unlocks more than one
 * account; everything else is free.
 *
 * Whether the user is Pro is asked to Google Play whenever the app comes to
 * the front (and retried if Google Play does not answer) and kept on the
 * phone, so the app knows it offline too. Debug builds count as Pro, so
 * development is never blocked (Play Billing only works for apps installed
 * from Play).
 */
@Singleton
class Billing @Inject constructor(
    @ApplicationContext private val context: Context,
) : PurchasesUpdatedListener {
    private val prefs = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    private val _pro = MutableStateFlow(BuildConfig.DEBUG || prefs.getBoolean(KEY_PRO, false))

    /** The user can use more than one account. */
    val pro: StateFlow<Boolean> = _pro.asStateFlow()

    private val _subscribed = MutableStateFlow(prefs.getBoolean(KEY_PRO, false))

    /** A real Google Play subscription exists (debug builds aside): it can be managed. */
    val subscribed: StateFlow<Boolean> = _subscribed.asStateFlow()

    private val _price = MutableStateFlow<String?>(null)

    /** The yearly price as Google Play shows it to this user ("9,99 €"), once known. */
    val price: StateFlow<String?> = _price.asStateFlow()

    private var details: ProductDetails? = null

    private val main = Handler(Looper.getMainLooper())
    @Volatile private var connecting = false
    private var retryMs = FIRST_RETRY_MS

    private val client: BillingClient = BillingClient.newBuilder(context)
        .setListener(this)
        .enablePendingPurchases(PendingPurchasesParams.newBuilder().enableOneTimeProducts().build())
        .enableAutoServiceReconnection()
        .build()

    /**
     * Connects to Google Play and refreshes the state: when the app comes to
     * the front and when the offer is shown. Safe to call often.
     */
    fun start() {
        // Demo mode: Google Play is not asked (debug builds count as Pro).
        if (com.ferrowl.app.demo.Demo.on) return
        if (client.isReady) {
            refresh()
            return
        }
        if (connecting) return
        connecting = true
        client.startConnection(object : BillingClientStateListener {
            override fun onBillingSetupFinished(result: BillingResult) {
                connecting = false
                if (result.responseCode == BillingClient.BillingResponseCode.OK) {
                    retryMs = FIRST_RETRY_MS
                    refresh()
                } else {
                    retryLater()
                }
            }

            // Reconnection happens on the next call (enableAutoServiceReconnection).
            override fun onBillingServiceDisconnected() {
                connecting = false
            }
        })
    }

    /** Google Play unavailable (updating, offline): tries again, waiting longer each time. */
    private fun retryLater() {
        main.removeCallbacksAndMessages(null)
        main.postDelayed({ start() }, retryMs)
        retryMs = (retryMs * 2).coerceAtMost(MAX_RETRY_MS)
    }

    /** Asks Google Play for the subscription and its price. */
    private fun refresh() {
        client.queryPurchasesAsync(
            QueryPurchasesParams.newBuilder().setProductType(BillingClient.ProductType.SUBS).build(),
        ) { result, purchases ->
            if (result.responseCode == BillingClient.BillingResponseCode.OK) apply(purchases) else retryLater()
        }
        val product = QueryProductDetailsParams.Product.newBuilder()
            .setProductId(PRODUCT)
            .setProductType(BillingClient.ProductType.SUBS)
            .build()
        client.queryProductDetailsAsync(QueryProductDetailsParams.newBuilder().setProductList(listOf(product)).build()) { result, found ->
            if (result.responseCode != BillingClient.BillingResponseCode.OK) return@queryProductDetailsAsync
            details = found.productDetailsList.firstOrNull()
            _price.value = baseOffer(details)?.pricingPhases?.pricingPhaseList?.lastOrNull()?.formattedPrice
        }
    }

    /** Opens Google Play's purchase sheet. False when the subscription is not available. */
    fun buy(activity: Activity): Boolean {
        val product = details
        val offer = baseOffer(product)?.offerToken
        if (product == null || offer == null) {
            start() // maybe Google Play did not answer yet
            return false
        }
        val params = BillingFlowParams.newBuilder()
            .setProductDetailsParamsList(
                listOf(
                    BillingFlowParams.ProductDetailsParams.newBuilder()
                        .setProductDetails(product)
                        .setOfferToken(offer)
                        .build(),
                ),
            )
            .build()
        return client.launchBillingFlow(activity, params).responseCode == BillingClient.BillingResponseCode.OK
    }

    /** Opens Google Play's page of this subscription (cancel, change payment). */
    fun manage(context: Context) {
        val uri = "https://play.google.com/store/account/subscriptions?sku=$PRODUCT&package=${context.packageName}".toUri()
        context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }

    override fun onPurchasesUpdated(result: BillingResult, purchases: MutableList<Purchase>?) {
        when (result.responseCode) {
            BillingClient.BillingResponseCode.OK -> {
                // Only adds: a cancelled sheet must not take Pro away.
                val pro = purchases.orEmpty().filter(::isActivePro)
                pro.forEach(::acknowledge)
                if (pro.isNotEmpty()) setPro(true)
            }
            // Bought before (another phone, reinstall): ask Google Play again.
            BillingClient.BillingResponseCode.ITEM_ALREADY_OWNED -> refresh()
            else -> Unit
        }
    }

    /** The yearly base plan (not a special offer, whose order is not guaranteed). */
    private fun baseOffer(product: ProductDetails?) =
        product?.subscriptionOfferDetails?.let { offers -> offers.firstOrNull { it.offerId == null } ?: offers.firstOrNull() }

    /** The full list of the user's subscriptions: decides Pro either way. */
    private fun apply(purchases: List<Purchase>) {
        val active = purchases.filter(::isActivePro)
        active.forEach(::acknowledge)
        setPro(active.isNotEmpty())
    }

    private fun isActivePro(p: Purchase) =
        PRODUCT in p.products && p.purchaseState == Purchase.PurchaseState.PURCHASED

    /** Google refunds purchases not acknowledged within three days. */
    private fun acknowledge(p: Purchase) {
        if (p.isAcknowledged) return
        client.acknowledgePurchase(AcknowledgePurchaseParams.newBuilder().setPurchaseToken(p.purchaseToken).build()) { }
    }

    private fun setPro(on: Boolean) {
        prefs.edit { putBoolean(KEY_PRO, on) }
        _subscribed.value = on
        _pro.value = BuildConfig.DEBUG || on
    }

    companion object {
        /** The subscription's product ID in Play Console (yearly base plan). */
        const val PRODUCT = "ferrowl_pro"
        private const val FILE = "billing"
        private const val KEY_PRO = "pro"
        private const val FIRST_RETRY_MS = 15_000L
        private const val MAX_RETRY_MS = 15 * 60_000L
    }
}
